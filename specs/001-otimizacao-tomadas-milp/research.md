# Phase 0 — Research & Decisions

Feature: Otimização Energética de Tomadas Inteligentes (Modelo MILP Multi-Nó)
Date: 2026-05-29 | Branch: `001-otimizacao-tomadas-milp`

Resolve os itens deferidos na spec/constituição. Cada item: Decisão / Justificativa /
Alternativas consideradas.

---

## §1. Integralidade binária com `minilp` (LP → MILP)

**Contexto**: O Princípio I exige `x_{i,s} ∈ {0,1}`. O crate `minilp` resolve apenas
LP contínuo (relaxação). A constituição marcou isto como risco a resolver aqui.

**Decisão**: Implementar uma camada de **branch-and-bound** própria sobre o
`minilp` no crate `tomada-solver`:
1. Resolver a relaxação LP com `minilp` (variáveis em `[0,1]`).
2. Se a solução já for inteira, retorná-la.
3. Caso contrário, ramificar na variável fracionária mais próxima de 0,5, fixando
   `x = 0` e `x = 1`, e recursar com poda por *bound* (objetivo).
4. As folgas `y_uso`, `y_age` permanecem contínuas (`≥ 0`) — não precisam ramificar.

O espaço é minúsculo (`N ≤ 8`, 3 estados ⇒ ≤ 24 binárias, e R1 reduz a 3^8 = 6561
combinações no pior caso), então B&B converge instantaneamente no ESP32. Isto satisfaz
o Princípio I (integralidade) sem violar o Princípio II (a complexidade vem do
acoplamento `P_max`, não do método).

**Alternativas**:
- *Enumeração exaustiva* (3^N): viável para `N ≤ 8`, mas não escala e não reaproveita a
  estrutura LP; mantida como fallback de validação cruzada nos testes.
- *Crate `good_lp` com backend MILP (CBC/HiGHS)*: inviável no Xtensa (depende de libs C
  pesadas); rejeitado por incompatibilidade embarcada.
- *Heurística gulosa por `Π`/consumo*: rápida mas não garante ótimo nem factibilidade
  formal; rejeitada (fere Princípio I).

---

## §2. Coordenação multi-nó: ESP-NOW vs. mediada por Supabase

**Contexto**: A spec define **ESP-NOW** como canal entre ESP32. O `supabase.rs.template`
de referência coordena via nuvem (`buscar_todas_tomadas()` lê `p_medida_atual` de todas
as tomadas). É preciso reconciliar para honrar `P_max` (R2) de forma resiliente.

**Decisão**: **ESP-NOW como canal primário** de coordenação do teto global, com Supabase
apenas como persistência/configuração (não no caminho crítico da decisão):
- Cada tomada difunde periodicamente (ESP-NOW broadcast) seu `p_medida_atual`, estado
  atual e flags `U_i`/`F_i`.
- Cada nó mantém em RAM uma tabela dos peers (id → última leitura + timestamp). Um peer
  sem heartbeat por `T_peer` (ex.: 3 ciclos) é considerado indisponível.
- Cada nó monta o `SolverInput` multi-tomada a partir dessa tabela e resolve o MILP
  localmente (decisão idêntica entre nós pois o modelo é determinístico e as entradas
  são compartilhadas).
- **Degradação segura** (Princípio V): se a tabela de peers estiver incompleta, o nó
  assume o consumo dos peers ausentes como o **último valor conhecido** (ou o pior caso
  `C_{i,ON}` se nunca visto), garantindo que não viole o `P_max` conhecido.

Supabase continua recebendo medições/eventos/estado em lote (fonte de verdade,
dashboard, histórico de `Π`). Isto mantém offline-first: a decisão funciona sem nuvem.

**Justificativa**: ESP-NOW é local, baixa latência e independe de Wi-Fi/internet —
alinhado ao offline-first (Princípio VI) e à resiliência (Princípio V). A coordenação
por nuvem teria latência e dependeria de conectividade no caminho crítico.

**Alternativas**:
- *Coordenação só por Supabase* (como o template): rejeitada por depender da nuvem para
  respeitar `P_max` (frágil em queda de internet).
- *Coordenador central eleito (um nó resolve e comanda os demais)*: mais simples de
  raciocinar, porém ponto único de falha; rejeitado em favor de decisão replicada
  determinística. (Pode ser reavaliado se sincronização de relógio entre nós divergir.)

**Ponto de atenção para `/tasks`**: garantir entradas idênticas entre nós (mesma janela
de tempo `t` via SNTP) para que a decisão replicada seja consistente.

---

## §3. Leitura dos sensores (ZMPT101B + SCT-013 20A/1V) e derivação de `U_i`/`F_i`

**Decisão**:
- Amostrar os dois canais ADC do ESP32 em alta frequência por uma janela curta
  (ex.: ~1–2 s, várias centenas de amostras) e calcular **RMS** de tensão e corrente;
  potência aparente `P = Vrms · Irms` (suficiente para o modelo; fator de potência fora
  de escopo).
- SCT-013 20A/1V: saída em tensão (1 V ↔ 20 A) → ler direto no ADC com divisor/bias.
  ZMPT101B: saída em tensão proporcional, com offset; calibrar ganho por constante.
- **`U_i`** (uso ativo) = `P_atual > p_limiar_i`. O firmware autodetecta um limiar
  (`p_limiar_detectado_auto`) e o publica; o usuário pode sobrepor.
- **`F_i`** (fantasma) = tomada energizada porém ociosa (`P` baixo, entre `cstd` e
  `p_limiar`) por tempo > `t_limite_i`.
- **Calibração** `cstd`/`con`: medir consumo típico em standby e ativo por tomada;
  alimenta `C_{i,STD}`/`C_{i,ON}` (e `C_{i,OFF}=0`).

**Justificativa**: RMS sobre janela em RAM é barato e não desgasta flash (Princípio
III); casa com os campos já previstos no `supabase.rs.template` (`cstd`, `con`,
`p_medida_atual`, `ultimo_uso_ativo_ts`, `p_limiar_detectado_auto`).

**Alternativas**: amostragem contínua + FFT/potência real — rejeitada por custo de CPU
sem impacto na decisão (o modelo usa só ordenação de consumo, conforme crítica do PDF).

---

## §4. Identidade e Autenticação ESP32 ↔ Supabase (Princípio VIII)

**Contexto**: O template original usava uma *anon key* compartilhada, hardcoded no
`.env`/`.bin`, com RLS "a definir". Isso é uma falha de segurança: a chave é idêntica em
todos os nós e extraível com `strings` de um único `.bin` — quem a obtém lê e escreve os
dados de **todas** as residências, inclusive `rele_aberto` e `p_max` (acionam relé e
teto elétrico). A Constituição v1.2.0 (Princípio VIII) proíbe esse modelo.

**Decisão** (sessão 2026-05-29): **identidade por MAC→UUID + JWT por-dispositivo + RLS
default-deny**, com provisionamento via Edge Function. Detalhes em
`contracts/provisioning-api.md` e `contracts/supabase-schema.sql`.

- **Identidade**: o MAC do efuso (`esp_efuse_mac_get_default`) é a identidade de
  hardware (coluna `tomadas.mac` única), mapeada a um `tomada_id` UUID. MAC **identifica,
  não autentica** (é público no Wi-Fi/ESP-NOW).
- **Autenticação**: cada device recebe um **segredo durável** (refresh) + um **JWT curto**
  (role `device`, claims `residencia_id`/`tomada_id`). O firmware fala com o PostgREST com
  `Authorization: Bearer <jwt>`; renova via endpoint `renovar` quando perto de expirar.
- **Isolamento**: RLS **default-deny** em todas as tabelas; policies chaveiam em
  `auth.jwt()->>'residencia_id'`/`'tomada_id'`. Menor privilégio por coluna: `device`
  não escreve `rele_aberto`/`p_max`/`mac`. Dashboard usa role `authenticated`
  (isolado por `owner_id = auth.uid()`).
- **Segredo de assinatura do JWT nunca na borda**: emissão/renovação só na Edge Function
  (`service_role`). `.bin` genérico, sem credencial por-dispositivo.
- **Provisionamento**: portal AP coleta Wi-Fi + claim-code → `provisionar(mac, claim)`
  registra a tomada e devolve segredo+JWT → gravados em LittleFS.
- TLS mantido: cert bundle global do ESP-IDF (`CONFIG_MBEDTLS_CERTIFICATE_BUNDLE=y`).

**Justificativa**: contém o raio de explosão a um único dispositivo (vazar um `.bin` não
expõe outras residências), habilita rotação/revogação sem reflash e protege as escritas
safety-critical. O custo (uma Edge Function + claim-code) é pequeno e isolado — não fere
o Princípio II, pois remove uma falha de *safety*, não adiciona complexidade gratuita.

**Alternativas**:
- *Anon key compartilhada + RLS por residência* (template original): **rejeitada** — a
  chave única não permite isolamento real entre devices/residências e vaza no `.bin`.
- *service-role key no firmware*: rejeitada (credencial irrestrita exposta).
- *Supabase Auth (1 usuário por device)*: viável, mas gerir N usuários GoTrue é mais
  pesado que assinar JWT com claims na Edge Function; preterido pela simplicidade.

---

## §5. Offline-first: fila de envio e reconciliação

**Decisão**: Medições de 1 min entram numa **fila local** (LittleFS, append em lote).
Um sincronizador envia em lote ao Supabase quando há conectividade; registros recebem
marca de "confirmado" após HTTP 2xx. Comandos do dashboard (`rele_aberto`) e config
(`config_version`) são lidos do Supabase quando online (pull). A decisão do solver nunca
depende da nuvem.

**Justificativa**: Princípios VI (offline-first) e III (lote, não contínuo).

**Alternativas**: envio síncrono por medição — rejeitado (tráfego/flash, fere III).

---

## §6. Retenção e purga local (FR-022/FR-023)

**Decisão**: Implementar `persistencia::retencao`:
- A purga roda **a cada 5 dias**; remove apenas registros marcados como confirmados na
  nuvem.
- Qualquer registro com idade > **7 dias** é removido mesmo sem confirmação (cap rígido
  de proteção da flash). A perda do dado mais antigo é aceita em outage > 7 dias.
- Implementação por **segmentação temporal** (arquivos/baldes por dia) para que a purga
  seja um `remove` de balde inteiro — barato e evita reescrever arquivos grandes
  (minimiza desgaste, Princípio III).

**Justificativa**: casa exatamente com a clarificação FR-023 (Opção A) e protege a
flash. Segmentar por dia torna a purga O(1) por balde.

**Alternativas**: arquivo único com compactação — rejeitado (reescrita custosa, mais
escrita em flash).

---

## §7. Modelo `Π_{i,t}` horário (FR-013)

**Decisão**: `pi_historico` é um vetor de **24 floats** (uma probabilidade por hora do
dia), já previsto no `supabase.rs.template`. Atualizado por um agregador que, a cada
dia, recalcula por hora a fração de uso (`U_i`) observada na janela dos últimos `D_i`
dias. O cálculo pode rodar no edge (firmware) ou ser derivado de `medicoes` no
Supabase/dashboard; para o TCC, **edge** com publicação ao Supabase (campo `config`).

**Justificativa**: 24 floats é leve; alinhado à granularidade horária decidida (Q2) e
ao campo existente.

**Alternativas**: 168 faixas (dia×hora) — fora do escopo decidido (Q2 = Opção A).

---

## Resumo das decisões

| # | Tema | Decisão |
|---|------|---------|
| 1 | Solver | minilp + branch-and-bound próprio (binário) |
| 2 | Multi-nó | ESP-NOW primário; Supabase persistência; degradação segura |
| 3 | Sensores | RMS em janela; `P=Vrms·Irms`; `U_i`/`F_i`/calibração |
| 4 | Identidade/Auth | MAC→UUID + JWT por-device + RLS default-deny; provisionamento via Edge Function |
| 5 | Offline | fila local em lote + reconciliação por confirmação |
| 6 | Retenção | purga 5d (confirmados) + cap rígido 7d; baldes por dia |
| 7 | `Π` | vetor de 24 horas, janela `D_i` dias, calculado no edge |

Todas as NEEDS CLARIFICATION do Technical Context foram resolvidas. Pronto para Phase 1.
