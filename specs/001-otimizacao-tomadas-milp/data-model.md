# Phase 1 — Data Model

Feature: Otimização Energética de Tomadas Inteligentes (Modelo MILP Multi-Nó)
Date: 2026-05-29 | Branch: `001-otimizacao-tomadas-milp`

Entidades do domínio, mapeadas para (a) o crate `tomada-solver`, (b) o estado em RAM/
LittleFS do firmware e (c) as tabelas do Supabase (ver `contracts/supabase-schema.sql`).
Notação do modelo conforme `review_modelo.pdf` (tópicos 1–4).

---

## 1. Estado operacional (`Estado`)

Enum fechado (conjunto `S`):

| Valor | Código | Relé (Opção A) | Significado |
|-------|--------|----------------|-------------|
| OFF | 0 | aberto (LOW) | energia cortada, consumo nulo |
| STD | 1 | fechado (HIGH) | energizado, aparelho em standby (fantasma) |
| ON  | 2 | fechado (HIGH) | energizado, uso ativo |

Regra de atuação: `rele_aberto = (estado == OFF)`.

---

## 2. Tomada / Nó (`Tomada`) — config + telemetria

Persistida em `tomadas` (Supabase) e espelhada em LittleFS (`persistencia::config`).
Campos já previstos no `supabase.rs.template`.

| Campo | Tipo | Origem | Mapeia para |
|-------|------|--------|-------------|
| `id` | uuid/string | atribuído (provisionamento) | índice `i ∈ I` |
| `mac` | string | efuso (`esp_efuse_mac_get_default`) | identidade de hardware (Princípio VIII); única |
| `residencia_id` | uuid/string | config | agrupa o conjunto `I` |
| `is_critical` | bool | usuário ("não pode desligar") | `W_i = M` quando true |
| `w_i` | int | derivado/config | peso de criticidade `W_i` |
| `p_max` | int (W) | config | teto global `P_max` (igual p/ a residência) |
| `t_limite` | int (s) | config | `T_limite,i` (limiar de ociosidade → `F_i`) |
| `schedule` | bool[24] | usuário | agendamento `A_{i,t}` (1=permitido por hora) |
| `pi_historico` | float[24] | edge (histórico) | `Π_{i,t}` por hora |
| `p_limiar_detectado_auto` | float (W) | firmware | `P_limiar,i` (→ `U_i`) |
| `config_version` | int | firmware/dashboard | controle de sync (pull) |
| `cstd` | float (W) | calibração | `C_{i,STD}` |
| `con` | float (W) | calibração | `C_{i,ON}` (e `C_{i,OFF}=0`) |
| `p_medida_atual` | float (W) | medição | leitura corrente (compartilhada via ESP-NOW) |
| `ultimo_uso_ativo_ts` | int64 | medição | base p/ `F_i` |
| `estado` | enum | solver | estado decidido |
| `rele_aberto` | bool | dashboard/solver | comando manual (FR-010a) + atuação |

**Validações**: `0 ≤ pi_historico[h] ≤ 1`; `schedule.len() == 24`;
`pi_historico.len() == 24`; `cstd ≤ con`; `p_max > 0`; `w_i ≥ 1` (crítico ⇒ `w_i = M`).

**Transições de estado**: decididas pelo solver a cada ciclo; `rele_aberto` manual do
dashboard tem prioridade de comando, mas nunca viola `P_max` (FR-010a).

---

## 2bis. Identidade e Credenciais do Dispositivo (Princípio VIII)

Separação **identidade ≠ autenticação**. Detalhes em `contracts/provisioning-api.md` e
`contracts/supabase-schema.sql`.

**Identidade do dispositivo (`CredenciaisLocais`)** — persistida em LittleFS
(`persistencia::credenciais`), provisionada em runtime (nunca no `.bin`):

| Campo | Tipo | Origem | Notas |
|-------|------|--------|-------|
| `mac` | string | efuso | identidade de hardware; igual a `tomadas.mac` |
| `tomada_id` | uuid/string | Edge Function | `i ∈ I`; claim do JWT |
| `residencia_id` | uuid/string | Edge Function | claim do JWT |
| `device_secret` | string (≥32 bytes) | Edge Function (`provisionar`) | **segredo durável** (refresh); só hash no servidor |
| `jwt` | string | Edge Function | token de regime (RAM); renovável via `renovar` |
| `jwt_exp` | int64 | JWT | epoch de expiração; renovar antes |

**Servidor (Supabase, acessível só por `service_role`):**

- **`device_credentials`**: `tomada_id` (PK), `secret_hash = sha256(device_secret)`,
  `revoked_at`. Revogar = `revoked_at != null`. Nunca exposta a `device`/`authenticated`.
- **`provisioning_claims`**: `residencia_id`, `code_hash = sha256(claim_code)`,
  `expira_em`, `usado_em` (uso único). Gerado pelo dono no dashboard.

**Validações/Regras**:
- `mac` único e imutável por chip; `device_secret` aleatório CSPRNG; comparado por hash
  em tempo constante.
- O firmware envia `Authorization: Bearer <jwt>`; ao receber `401` ou perto de `jwt_exp`,
  chama `renovar(tomada_id, device_secret)`.
- O `device` (RLS) só **escreve** a própria linha de `tomadas` e nunca `rele_aberto`,
  `p_max`, `mac` (menor privilégio por coluna).

---

## 3. Medição (`Medicao`)

Buffer em RAM → LittleFS (baldes por dia) → Supabase `medicoes`. Cadência: 1/min
(FR-016), gravação em lote (FR-016b).

| Campo | Tipo | Notas |
|-------|------|-------|
| `tomada_id` | string | FK → tomadas |
| `timestamp` | int64 | epoch (SNTP) |
| `p_medida` | float (W) | potência agregada do minuto |
| `estado` | enum | estado vigente no minuto |
| `rele_aberto` | bool | atuação vigente |
| `confirmado` | bool (local) | true após 2xx do Supabase (controle de purga) |

**Validações**: `p_medida ≥ 0`; `timestamp` monotônico por tomada.
**Retenção**: purga a cada 5 dias dos `confirmado=true`; cap rígido 7 dias.

---

## 4. Evento (`Evento`) — observabilidade (Princípio VII)

Supabase `eventos`. Registra decisões e violações de regra (penalidades).

| Campo | Tipo | Notas |
|-------|------|-------|
| `tomada_id` | string | FK → tomadas |
| `timestamp` | int64 | epoch |
| `tipo` | string | `chaveamento`, `penalidade_uso` (R3), `penalidade_age` (R5), `fantasma_eliminado` (R4), `solver` |
| `severidade` | string | `info`/`warn`/`erro` |
| `titulo` | string | resumo |
| `detalhes` | string/json | entradas, `Z`, estados, folgas ativadas (reprodutível) |

---

## 5. Entrada/Saída do solver (`tomada-solver`)

Estruturas puras (host-testáveis). Contrato completo em `contracts/solver-api.md`.

> Origem no firmware: a própria tomada preenche seu `TomadaInput` da config local +
> sensores; cada peer é preenchido a partir do `HeartbeatTomada` (ESP-NOW), que carrega
> `c_std`, `c_on`, `u_i`/`f_i`/`a_it` (flags), `pi_hora` e `w_i`. Peer ausente vira
> carga fixa subtraída do `P_max` (ver `contracts/espnow-protocol.md`).

**`SolverInput`** (snapshot multi-tomada para o instante `t`):
- `p_max: f64` — teto global `P_max`.
- `hora: u8` — faixa horária `t` (0–23), p/ indexar `Π` e `schedule`.
- `pesos: { alpha, beta, gamma }` — coeficientes da objetivo (β,γ ≫ 1).
- `tomadas: Vec<TomadaInput>`, cada uma com:
  - `c_off=0, c_std, c_on` — `C_{i,s}`.
  - `u_i: bool` (uso ativo), `f_i: bool` (fantasma).
  - `pi: f64` — `Π_{i,t}` da hora.
  - `w_i: f64` — criticidade.
  - `a_it: bool` — `schedule[hora]`.

**`SolverOutput`**:
- `estados: Vec<Estado>` — `x_{i,s}` resolvido (um estado por tomada).
- `z: f64` — valor da função objetivo.
- `penalidades: Vec<{ tomada_idx, tipo, valor }>` — folgas `y_uso`/`y_age` ativadas.
- `factivel: bool` — sempre `true` por construção (folgas), mas exposto p/ asserção.

---

## 6. Relação com a formulação (rastreabilidade)

| Modelo (PDF) | Onde vive |
|--------------|-----------|
| `I` (conjunto de tomadas) | `tomadas` por `residencia_id` |
| `S = {OFF,STD,ON}` | `Estado` |
| `C_{i,s}` | `cstd`/`con` (OFF=0) |
| `P_max` (R2) | `tomadas.p_max` |
| `U_i` (R3) | derivado de `p_medida_atual` vs `p_limiar` |
| `F_i` (R4) | derivado de ociosidade vs `t_limite` |
| `Π_{i,t}` | `pi_historico[24]` |
| `W_i`, crítico | `w_i`, `is_critical` |
| `A_{i,t}` (R5) | `schedule[24]` |
| `x_{i,s}` | `SolverOutput.estados` / `tomadas.estado` |
| `y_uso`, `y_age` | `SolverOutput.penalidades` → `eventos` |
| `Z` | `SolverOutput.z` → `eventos.detalhes` |
