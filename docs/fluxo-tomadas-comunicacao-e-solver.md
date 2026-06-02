# Fluxo do sistema: provisionamento, dashboard, ESP-NOW e comportamento do solver

> Documento de referência (TCC — tomadas inteligentes ESP32/Rust). Tudo aqui está
> ancorado no código real:
> - `firmware/src/comunicacao/provisionamento.rs` · `supabase.rs` · `espnow.rs`
> - `firmware/src/decisao/ciclo.rs`
> - `supabase/functions/provisionar/index.ts`
> - `tomada-solver/` (modelo MILP) + `tomada_solver::sync`
> - `dashboard/` (Vite + TS) lendo as tabelas `tomadas` / `medicoes` / `eventos`.

---

## 1. Conectar uma tomada ESP32 e ela aparecer no dashboard

### 1.1 Ciclo de vida (do plug ao card)

```
┌─ ESP32 (1º boot) ─────────────┐
│ 1. Conecta WiFi               │   firmware/.env (WIFI_SSID/PASS) ou portal AP
│ 2. Lê MAC do efuse            │   provisionamento::obter_mac()
│ 3. POST /functions/v1/        │   {mac, claim_code}
│    provisionar                │
└───────────────┬───────────────┘
                ▼
┌─ Edge Function `provisionar` (service_role; ÚNICO que assina JWT) ────────┐
│ • resolve a residência (claim_code  OU  modo AUTO=demo)                   │
│ • UPSERT em `tomadas` pelo MAC  →  CRIA A LINHA (nome="Tomada XXXXX")     │ ← a tomada "nasce"
│ • gera device_secret (guarda só o sha256)                                 │
│ • assina o 1º JWT ES256 (role=device, claims tomada_id + residencia_id)   │
│ • devolve: tomada_id, residencia_id, device_secret, residencia_key,       │
│   jwt, jwt_exp                                                            │
└───────────────┬───────────────────────────────────────────────────────────┘
                ▼
┌─ ESP32 grava tudo no LittleFS (CredenciaisLocais) ────────────────────────┐
│ A partir daqui só fala com o Supabase com o JWT do device                 │
│ (renovado sozinho por provisionamento::obter_jwt_valido)                  │
└───────────────┬───────────────────────────────────────────────────────────┘
                ▼
┌─ Loop de regime (firmware publica via PostgREST) ─────────────────────────┐
│ medições em lote → `medicoes`        (supabase::sincronizar_medicoes)      │
│ estado/cstd/con/p_medida_atual → PATCH `tomadas` (atualizar_*)             │
│ eventos do solver → `eventos`        (supabase::sincronizar_eventos)       │
└───────────────┬───────────────────────────────────────────────────────────┘
                ▼
┌─ DASHBOARD ───────────────────────────────────────────────────────────────┐
│ fetchTomadas(residencia_id) → sidebar + cards                             │
│ fetchMedicoes + realtime    → gráficos / mini-barras                      │
│ fetchEventos                → "Atividade do Otimizador"                    │
└───────────────────────────────────────────────────────────────────────────┘
```

### 1.2 Caminho de volta (dashboard → tomada)

O dashboard não só lê; ele escreve o **estado desejado**, que o firmware puxa em
`supabase::buscar_desired_state()`:

| Ação no dashboard      | Escreve em                         | Firmware lê via          | Efeito |
|------------------------|------------------------------------|--------------------------|--------|
| Toggle Controle Manual | `tomadas.rele_aberto`              | `buscar_desired_state()` | liga/desliga o relé (subordinado ao P_max) |
| Salvar Agendamento/Crítica | `tomadas.config` + `config_version` | mesmo GET           | adota só se `config_version` remoto > local (`deve_adotar_config`) |
| Slider P_max           | `residencias.p_max`                | —                        | teto elétrico do circuito |

### 1.3 Os dois modos de "claim" (`supabase/functions/provisionar/index.ts`)

- **Modo AUTO** (`AUTO_PROVISION=true`, default da demo): a tomada manda `claim_code`
  vazio → cai numa residência padrão (`DEFAULT_RESIDENCIA_ID`, ou a 1ª existente, ou
  cria "Casa Demo"). "Conecta e usa", sem login. **Sem** isolamento multi-inquilino.
- **Modo claim seguro** (`AUTO_PROVISION=false`): exige um `claim_code` (≥6 chars) ligado
  a uma residência específica (`provisioning_claims`, uso único, expira). É o
  `CLAIM_CODE=` do `firmware/.env`.

### 1.4 ⚠️ Pré-condições para a tomada aparecer NESTE dashboard

A tomada só aparece se **tudo** isto bater:

1. **Mesmo projeto Supabase** em `firmware/.env` (`PROJECT_URL`) e em `dashboard/.env`
   (`VITE_SUPABASE_URL`) — incluindo a publishable key da mesma instância.
   > Hoje eles divergem: firmware → `mmsmfvzbcrkoscqnhvky`, dashboard →
   > `xlcxytwplxvglvvwysqo`. Se o device publica num projeto e o dashboard lê de outro,
   > **nada aparece**. Alinhar é o passo nº1.
2. **`VITE_RESIDENCIA_ID`** (dashboard) = a `residencia_id` em que a tomada foi
   provisionada (no modo AUTO, a residência padrão do projeto).
3. Schema + RLS + a chave de assinatura ES256 publicados nesse projeto (JWKS importado,
   senão a RLS rejeita o JWT do device).

---

## 2. Comunicação entre as tomadas (ESP-NOW v3)

A coordenação entre tomadas da **mesma casa** é peer-to-peer por rádio, **independente
da nuvem**. Implementação em `firmware/src/comunicacao/espnow.rs`.

### 2.1 Heartbeat (o "batimento" que cada tomada difunde)

- A cada ciclo, cada nó **difunde por broadcast** um `Heartbeat` de **44 bytes**
  (`TAM = 28 dados + 16 tag`).
- Conteúdo (campos que viram entrada do solver): `p_medida`, `estado`, flags
  (`U_i`, `F_i`, `rele_manual`, `A_it`), `c_std`, `c_on`, `pi_hora`, `w_i`, `epoch`.
- **Autenticação (protocolo v3):** cada heartbeat leva um **HMAC-SHA256 truncado (16 B)**
  calculado com a `residencia_key` (32 bytes, a MESMA para todos os nós da casa, entregue
  no provisionamento). Quem não tem a chave da casa não consegue forjar nem ler como
  válido → heartbeats de outra residência, forjados ou de replay são **descartados**.
- **Anti-replay:** o `epoch` precisa estar dentro de `FRESCOR_S = 60 s` do relógio local
  (relógios sincronizados via SNTP). Fora da janela → descartado.

### 2.2 Tabela de peers e classificação por ciclo

O callback de recepção autentica e guarda `(MAC → (Heartbeat, recebido_em))`.
No início de cada ciclo, `montar_coordenacao(agora, ttl_s)` classifica cada peer:

| Situação do peer                    | Vira…                                   | Por quê |
|-------------------------------------|------------------------------------------|---------|
| Heartbeat fresco (`≤ ttl_s`)        | **variável de decisão** `(MAC, TomadaInput)` | participa do MILP |
| Conhecido mas ausente (stale)       | **`carga_fixa`** = último `p_medida` dele | degradação segura |

A `carga_fixa` é **subtraída do `P_max`** no `SolverInput`. Isso garante que um peer que
sumiu **nunca seja subestimado** → o teto global conhecido **nunca é violado** por
otimismo.

### 2.3 Ordenação canônica = mesma decisão em todos os nós

O ponto-chave da coordenação **sem um "mestre"**: em `ciclo.rs`, antes de resolver, o nó
junta `self + peers` e **ordena por MAC** (`com_mac.sort_by(... a.0.cmp(&b.0))`). Como
todos os nós recebem os mesmos heartbeats e ordenam pelo mesmo critério, **todos montam um
`SolverInput` idêntico**. O solver é **determinístico** (desempate por índice), então
todos chegam à **mesma alocação global** — e cada nó só aplica o estado do **seu próprio
índice** (`idx_proprio`). Não há negociação nem eleição de líder: é um MILP replicado.

---

## 3. Comportamento do solver em cada cenário

O solver (`tomada_solver::resolver`, MILP minilp + branch-and-bound) **roda localmente em
cada tomada** e sempre decide **apenas o estado da própria** (`OFF`/`STD`/`ON`),
respeitando o teto global `P_max` (R2) e as penalidades de uso ativo (R3) e agendamento
(R5). O que muda entre os cenários é **quantas tomadas entram no `SolverInput`** e **de
onde vêm os dados**.

### Cenário A — Tomada sozinha (sem peers, sem nuvem-no-loop)

- `peers_vivos = []`, `carga_fixa = 0`.
- `SolverInput` tem **1 tomada** e o **`P_max` inteiro** disponível.
- O solver decide o estado da própria tomada vs. seu teto: liga (`ON`/`STD`) se couber e
  fizer sentido pelo conforto/uso; corta para `OFF` se for fantasma (R4: energizada,
  ociosa além de `t_limite`) ou fora do horário permitido (R5).
- `main` trata `espnow::iniciar()` como **best-effort**: se falhar (1 placa só, rádio
  indisponível), o nó **opera sozinho** exatamente neste modo.

### Cenário B — Várias tomadas, todas vivas (coordenação ESP-NOW)

- Cada nó difunde seu heartbeat; cada nó monta `self + peers` ordenado por MAC.
- O `SolverInput` tem **N tomadas** e o **`P_max` compartilhado** (o teto é da casa, não
  por tomada).
- O MILP aloca **globalmente**: prioriza manter ligadas as de maior `w_i` (criticidade:
  geladeira/roteador) e maior probabilidade de uso `Π` na hora, e corta as de menor
  prioridade quando a soma estoura o `P_max`.
- Como a entrada é idêntica e o solver é determinístico, a soma das decisões **respeita o
  `P_max` sem dupla contagem nem conflito** — cada nó aplica só o seu índice.

### Cenário C — Peers conhecidos, mas algum sumiu (degradação segura)

- O peer ausente vira **`carga_fixa`** (último `p_medida` conhecido), subtraída do `P_max`.
- O solver otimiza as tomadas vivas dentro do **`P_max` reduzido**.
- Efeito prático: o sistema fica **conservador** — assume que a tomada sumida ainda pode
  estar consumindo, então não "gasta" essa folga. Nunca viola o teto por achar que há mais
  margem do que há.

### Cenário D — Sem conexão com o banco, só comunicação entre tomadas (OFFLINE)

Este é o ponto importante: **o laço de controle não depende da nuvem em regime.**

- **Pré-requisito (uma vez):** a tomada precisa ter sido provisionada online **ao menos
  uma vez** para ter `CredenciaisLocais` no LittleFS — em especial a **`residencia_key`**
  (sem ela não há ESP-NOW v3) e o `tomada_id`. Sem nunca ter provisionado, não há nem
  nuvem nem coordenação.
- **Com as credenciais já gravadas**, caindo a internet:
  - A coordenação ESP-NOW **continua 100%**: heartbeats, autenticação HMAC, ordenação
    canônica e o MILP replicado funcionam **sem o banco**. As tomadas continuam
    respeitando o `P_max` entre si.
  - A telemetria (`sincronizar_medicoes`/`sincronizar_eventos`) **acumula e re-tenta**:
    medições ficam `confirmado=false` na flash e sobem em lote idempotente
    (`on_conflict=tomada_id,timestamp`) quando a conexão volta — nada se perde.
  - O **comando manual do dashboard** e o **pull de config** ficam suspensos: como vêm de
    `buscar_desired_state()`, só chegam quando a nuvem volta. Até lá, cada nó segue a
    última config adotada + a decisão automática do solver.
  - O **fallback multi-nó pela nuvem** (`buscar_todas_tomadas`, usado quando o ESP-NOW
    está incompleto) fica indisponível — mas isso só importa se, **ao mesmo tempo**, o
    rádio também não cobrir os peers. Com ESP-NOW saudável, não é necessário.
- **Resumo:** offline = controle e coordenação preservados; o que pausa é a
  observabilidade ao vivo no dashboard e os comandos vindos dele (ambos se reconciliam ao
  reconectar).

### Cenário E — Comando manual vs. teto elétrico (vale em todos os cenários)

`ciclo.rs::aplicar_comando_manual` define a regra (FR-016):

- **Desligar** (`rele_aberto=true`): **sempre honrado** — cortar nunca viola o `P_max`.
- **Manter ligado** (`rele_aberto=false`): só honrado **se couber** no `P_max` disponível
  (descontada `carga_fixa` e o consumo dos outros). Se não couber, **o P_max vence**: a
  tomada fica `OFF` e registra um evento `penalidade_uso` ("Comando manual negado pelo
  P_max"), que aparece no feed do dashboard.

---

## 4. Mapa de dependências: o que precisa de nuvem × o que roda offline

| Função                                   | Precisa de nuvem? | Observação |
|------------------------------------------|-------------------|------------|
| 1º provisionamento (JWT + `residencia_key`) | **Sim** (1 vez) | sem isso não há ESP-NOW v3 nem nuvem |
| Renovação de JWT                         | Sim (quando perto de `exp`) | proativa; reativa em 401/403 |
| Decisão do solver (MILP)                 | **Não**           | local em cada nó |
| Coordenação ESP-NOW (P_max compartilhado)| **Não**           | rádio peer-to-peer autenticado |
| Telemetria/medições/eventos no dashboard | Sim (mas com fila offline) | lote idempotente; re-tenta |
| Comando manual + pull de config          | **Sim**           | via `buscar_desired_state()` |
| Fallback multi-nó por nuvem              | Sim               | só usado se ESP-NOW incompleto |

**Princípio de fundo:** a nuvem é a *fonte de verdade* e o painel/telecomando; o
**ESP-NOW é o sistema nervoso de tempo real** que mantém o `P_max` da casa mesmo sem
internet. O solver é o mesmo cérebro nos dois casos — só muda quantas tomadas ele enxerga
e de onde vêm os números.
