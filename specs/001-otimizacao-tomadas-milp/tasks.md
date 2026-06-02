---
description: "Task list for Otimização Energética de Tomadas Inteligentes (Modelo MILP Multi-Nó)"
---

# Tasks: Otimização Energética de Tomadas Inteligentes (Modelo MILP Multi-Nó)

**Input**: Design documents from `/specs/001-otimizacao-tomadas-milp/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/
(`solver-api.md`, `supabase-schema.sql`, `provisioning-api.md`, `espnow-protocol.md`)

**Tests**: Incluídos **apenas para o crate `tomada-solver`** (host-testável) porque o
contrato `contracts/solver-api.md` define 7 testes obrigatórios do modelo. O firmware é
validado por smoke tests on-device (não há testes automatizados de hardware).

**Organization**: Tarefas agrupadas por fase; user stories (P1→P3) em fases próprias para
implementação e teste independentes.

## Format: `[ID] [P?] [Story?] Description`

- **[P]**: Pode rodar em paralelo (arquivos diferentes, sem dependência pendente)
- **[Story]**: US1–US4 conforme spec.md (só nas fases de user story)
- Caminhos de arquivo são relativos à raiz do repo (`C:\y`)

## Path Conventions

- Crate do solver: `tomada-solver/src/`, `tomada-solver/tests/`
- Firmware: `firmware/src/`
- Edge Function: `supabase/functions/`
- Dashboard: `dashboard/src/`
- Banco/contratos: `specs/001-otimizacao-tomadas-milp/contracts/`

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Inicializar o workspace, o crate do solver e **furar os riscos de build**
antes de qualquer implementação (ver "Build Risks" abaixo).

- [x] T001 Criar `firmware/.cargo/config.toml` (target `xtensa-esp32-espidf`, `linker = ldproxy`, `runner = espflash flash --monitor`, `rustflags = ["--cfg","espidf_time64"]`, `[unstable] build-std = ["std","panic_abort"]`, `[env] MCU = "esp32"`). **Colocar em `firmware/` (não na raiz)** para que `cd firmware && cargo build` compile p/ ESP32 e `cargo test -p tomada-solver` rode no host.
- [ ] T002 Validar que o smoke test atual compila e flasheia: `cd firmware && cargo build` e `cargo run` (LittleFS+Wi-Fi+SNTP). **(requer ESP32 físico — pendente)**
- [x] T003 Adicionar o membro `tomada-solver` ao workspace em `Cargo.toml` (raiz)
- [x] T004 Criar o crate `tomada-solver` com `tomada-solver/Cargo.toml` (dep `minilp`, `serde`) e `tomada-solver/src/lib.rs` (módulos `tipos`, `modelo`, `solver`)
- [x] T005 [P] Adicionar a mitigação do bug LLVM Xtensa em `Cargo.toml` (raiz): `[profile.release.package.minilp] opt-level = 0` (mesmo padrão do `serde_json`)
- [ ] T006 **Spike de build do `minilp` no ESP32**: adicionar dep de path `tomada-solver` em `firmware/Cargo.toml`, resolver um LP trivial via `minilp` dentro do firmware e confirmar `cargo build` + `cargo run` no device. **Se falhar**: acionar o plano B do `research.md §1` (B&B sobre enumeração própria, sem `minilp`). **(requer ESP32 físico — pendente)** Nota: no host `minilp` compila e resolve OK (7 testes verdes).
- [ ] T007 [P] Adicionar a dep de ESP-NOW (`esp-idf-svc`/`esp-idf-sys`) em `firmware/Cargo.toml`

**Build Risks** (por que estas tarefas vêm primeiro):
- Falta `.cargo/config.toml` → sem ele `cargo build` mira o host e o `esp-idf-svc` quebra.
- `minilp` é float-pesado → risco do bug LLVM Xtensa `[2 x float]`; confirmar cedo (T006).
- Binário já ~2,4 MB de 3,125 MB → monitorar tamanho ao somar `minilp`+ESP-NOW+sensores.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Tipos, structs e schema compartilhados por TODAS as fases seguintes.

**⚠️ CRITICAL**: Nenhuma fase de user story nem de provisionamento começa antes desta.

- [x] T008 [P] Definir `Estado {Off,Std,On}`, `Pesos`, `TomadaInput`, `SolverInput`, `Penalidade`, `TipoPenalidade`, `SolverOutput` em `tomada-solver/src/tipos.rs` (conforme `contracts/solver-api.md`)
- [x] T009 [P] Definir helpers de parâmetros do modelo (`C_{i,s}`, indexação por hora) em `tomada-solver/src/modelo.rs`
- [ ] T010 [P] Criar struct `Tomada` (config) com serde: `schedule[24]`, `pi_historico[24]`, `w_i`, `p_max`, `t_limite`, `is_critical`, `p_limiar_detectado_auto`, `cstd`, `con`, `config_version` em `firmware/src/persistencia/config.rs`
- [ ] T011 [P] Criar struct `Medicao` (`tomada_id`, `timestamp`, `p_medida`, `estado`, `rele_aberto`, `confirmado`) em `firmware/src/persistencia/medicao.rs`
- [ ] T012 Aplicar o schema de `specs/001-otimizacao-tomadas-milp/contracts/supabase-schema.sql` no Supabase: tabelas (`residencias` c/ `owner_id`, `tomadas` c/ `mac` único, `medicoes`, `eventos`, `device_credentials`, `provisioning_claims`), role `device` + grants por coluna e **RLS default-deny** por claim (Princípios VI e VIII)
- [ ] T013 [P] Reduzir `firmware/src/config_nuvem.rs` a **só** `SUPABASE_URL` + Wi-Fi de bootstrap (REMOVER `SUPABASE_ANON_KEY`/`RESIDENCIA_ID`/`TOMADA_ID` do `.env`/binário — Princípio VIII); atualizar `firmware/.env.example`
- [ ] T014 Criar a árvore de módulos do firmware: `firmware/src/sensores/mod.rs`, `firmware/src/decisao/mod.rs`; registrar `pub mod sensores; pub mod decisao;` em `firmware/src/main.rs`; adicionar `pub mod supabase; pub mod espnow; pub mod provisionamento;` em `firmware/src/comunicacao/mod.rs` e `pub mod credenciais;` em `firmware/src/persistencia/mod.rs`

**Checkpoint**: Tipos, structs, schema e módulos prontos.

---

## Phase 3: Provisionamento e Autenticação (Princípio VIII) 🔒

**Purpose**: Identidade (MAC→`tomada_id`) + autenticação por **JWT por-dispositivo** com
**RLS default-deny**, sem segredo compartilhado no binário. Contratos:
`contracts/provisioning-api.md` e `contracts/supabase-schema.sql`.

**Blocking**: prerequisito de TODA escrita/leitura real no Supabase (US2 logging, US4
nuvem) e do teste multi-nó (US3). **Não** bloqueia o solver host-only (US1).

- [X] T015 Implementar a Edge Function `supabase/functions/provisionar/index.ts` (Deno): endpoints `provisionar(mac, claim_code)` e `renovar(tomada_id, device_secret)`; valida claim/segredo por hash (tempo constante), upsert de `tomadas` por `mac`, grava `device_credentials` (só `sha256` do segredo) e **assina o JWT** (claims `role=device`/`residencia_id`/`tomada_id`) com `PROJECT_JWT_SECRET` server-side; rate-limit por MAC/IP. ✅ código + README escritos. ⏳ deploy/teste requer Supabase CLI + projeto (`--no-verify-jwt`)
- [ ] T016 [P] Obter o MAC do efuso (`esp_efuse_mac_get_default` via `esp-idf-sys`) e formatar `AA:BB:CC:DD:EE:FF` em `firmware/src/comunicacao/provisionamento.rs`
- [ ] T017 [P] Criar `firmware/src/persistencia/credenciais.rs`: ler/gravar `{tomada_id, residencia_id, device_secret}` em LittleFS + cache do JWT (`jwt`, `jwt_exp`)
- [ ] T018 Estender `firmware/src/comunicacao/wifi_provisioning.rs` (portal AP) para coletar `claim_code`, chamar `provisionar` e persistir as credenciais via T017
- [ ] T019 Implementar renovação do JWT em `firmware/src/comunicacao/provisionamento.rs`: chamar `renovar(tomada_id, device_secret)` quando perto de `jwt_exp` ou ao receber `401`
- [ ] T020 Verificar isolamento RLS de ponta a ponta: JWT de uma residência NÃO lê/escreve dados de outra; role `device` não altera `rele_aberto`/`p_max`/`mac`; `anon` é negado (contra o schema aplicado em T012)

**Checkpoint**: Device se identifica pelo MAC e autentica com JWT por-device; RLS isola
por residência; `.bin` genérico, sem credencial embutida.

---

## Phase 4: User Story 1 - Decisão global sob teto P_max (Priority: P1) 🎯 MVP

**Goal**: Resolver o MILP que decide um estado por tomada minimizando consumo sob
`P_max` (R1+R2), com integralidade binária e sempre factível.

**Independent Test**: `cargo test -p tomada-solver` com entradas sintéticas; verificar um
estado por tomada, `Σ C ≤ P_max` e consumo mínimo. **Independe de auth/nuvem** (host).

### Tests for User Story 1 (contrato solver-api.md)

> Escrever os testes ANTES da implementação; garantir que falhem primeiro.

- [x] T021 [P] [US1] Teste de factibilidade: soma em ON excede `p_max` ⇒ retorna factível respeitando R2, em `tomada-solver/tests/modelo.rs`
- [x] T022 [P] [US1] Teste R2: desliga primeiro a tomada de menor `Π`; cross-check B&B == enumeração exaustiva (N≤6), em `tomada-solver/tests/modelo.rs`

### Implementation for User Story 1

- [x] T023 [US1] Implementar a construção LP: objetivo de consumo `Σ C_{i,s}·x_{i,s}` + R1 (unicidade) + R2 (capacidade, `≤ p_max`) com `minilp` em `tomada-solver/src/solver.rs`
- [x] T024 [US1] Implementar branch-and-bound para integralidade binária `x_{i,s}∈{0,1}` sobre a relaxação `minilp` (research §1) em `tomada-solver/src/solver.rs`
- [x] T025 [US1] Implementar `resolver(&SolverInput) -> SolverOutput` (estados, `z`, `factivel=true`) em `tomada-solver/src/solver.rs`
- [ ] T026 [P] [US1] Implementar o atuador do relé (Opção A: `Off`=set_low, `Std`/`On`=set_high) em `firmware/src/decisao/atuador.rs` **(firmware — requer device)**
- [ ] T027 [US1] Implementar o ciclo de decisão (montar `SolverInput` da config local, chamar `resolver`, aplicar o próprio estado via atuador) em `firmware/src/decisao/ciclo.rs` **(firmware)**

**Checkpoint**: Solver decide estados sob `P_max`; testável no host. MVP funcional.

---

## Phase 5: User Story 2 - Conforto preditivo e regras (Priority: P2)

**Goal**: Adicionar conforto preditivo (`α·Π·W·x_OFF`), R4 (hard) e R3/R5 (soft com
folgas), criticidade, e registro de penalidades — sempre factível.

**Independent Test**: `cargo test -p tomada-solver`: conflitos (uso ativo + horário
proibido) retornam solução válida com penalidades; `F_i` nunca gera `STD`; crítico
preservado.

### Tests for User Story 2 (contrato solver-api.md)

- [x] T028 [P] [US2] Teste R4: `f_i=true` ⇒ estado nunca `STD`, em `tomada-solver/tests/modelo.rs`
- [x] T029 [P] [US2] Teste R3: `u_i=true` forçado a OFF por `p_max` ⇒ `Penalidade::Uso` registrada, em `tomada-solver/tests/modelo.rs`
- [x] T030 [P] [US2] Teste R5: `a_it=false` com ativação necessária ⇒ `Penalidade::Agendamento`, em `tomada-solver/tests/modelo.rs`
- [x] T031 [P] [US2] Teste criticidade: `w_i=M` preserva a tomada sob pressão de `p_max`, em `tomada-solver/tests/modelo.rs`

### Implementation for User Story 2

- [x] T032 [US2] Adicionar termo de conforto `Σ α·Π_{i,t}·W_i·x_{i,OFF}` à objetivo e R4 (hard: `x_{i,STD}≤1−F_i`) em `tomada-solver/src/solver.rs`
- [x] T033 [US2] Adicionar R3 (`x_{i,OFF}≤1−U_i+y_uso`) e R5 (`x_{i,ON}≤A_{i,t}+y_age`) com folgas e termos `β·y_uso+γ·y_age` na objetivo em `tomada-solver/src/solver.rs`
- [x] T034 [US2] Preencher `SolverOutput.penalidades` (tipo, índice, valor) a partir das folgas ativadas em `tomada-solver/src/solver.rs`
- [ ] T035 [US2] Registrar decisões e penalidades em `eventos` (entradas + `Z` + estados + folgas, reprodutível) via cliente Supabase (Bearer JWT), chamado de `firmware/src/decisao/ciclo.rs` **(firmware; depende de Phase 3)**

**Checkpoint**: Modelo completo (tópicos 1–4) implementado e testado no host.

---

## Phase 6: User Story 3 - Coordenação multi-nó ESP-NOW (Priority: P2)

**Goal**: Cada ESP32 difunde seu snapshot e monta o MILP global a partir dos peers
(ESP-NOW), honrando `P_max` com degradação segura — sem depender da nuvem.

**Independent Test**: ≥2 ESP32 provisionados (cada um com seu `tomada_id` pelo MAC) na
mesma residência; decisão global respeita `P_max`; desligar um nó mantém decisão segura
(carga fixa pelo pior caso).

### Implementation for User Story 3

- [ ] T036 [P] [US3] Implementar serialização/desserialização do `HeartbeatTomada` (30 B, v2) + CRC conforme `contracts/espnow-protocol.md` em `firmware/src/comunicacao/espnow.rs`
- [ ] T037 [US3] Inicializar ESP-NOW (broadcast/peers, `N≤8`) e envio periódico do próprio heartbeat por ciclo em `firmware/src/comunicacao/espnow.rs`
- [ ] T038 [US3] Manter tabela de peers em RAM (`id → último heartbeat + ts`) com liveness `T_peer` (3 ciclos) em `firmware/src/comunicacao/espnow.rs`
- [ ] T039 [US3] No ciclo, montar `SolverInput` multi-tomada (eu + peers vivos); peer ausente entra como carga fixa subtraída do `P_max` em `firmware/src/decisao/ciclo.rs`
- [ ] T040 [US3] Calcular e publicar no heartbeat os parâmetros do ciclo da própria tomada (`pi_hora`, `w_i`, `a_it`, `U_i`, `F_i`, `cstd`, `con`) em `firmware/src/comunicacao/espnow.rs`

**Checkpoint**: Decisão global coordenada e resiliente entre nós, offline-first.

---

## Phase 7: User Story 4 - Medição, persistência, retenção e nuvem (Priority: P3)

**Goal**: Medir tensão/corrente, derivar `U_i`/`F_i`/calibração, persistir 1/min em
lote, sincronizar com Supabase (Bearer JWT) e aplicar retenção (5d/7d).

**Independent Test**: Gerar medições por vários dias; confirmar persistência na nuvem e
purga sem perder dados confirmados; dados locais nunca > 7 dias.

### Implementation for User Story 4

- [ ] T041 [P] [US4] Ler tensão RMS do ZMPT101B (ADC, janela em RAM) em `firmware/src/sensores/zmpt101b.rs`
- [ ] T042 [P] [US4] Ler corrente RMS do SCT-013 20A/1V (ADC, janela em RAM) em `firmware/src/sensores/sct013.rs`
- [ ] T043 [US4] Calcular `P=Vrms·Irms`, derivar `U_i` (vs `p_limiar` auto), `F_i` (ociosidade vs `t_limite`) e calibrar `cstd`/`con` em `firmware/src/sensores/potencia.rs`
- [ ] T044 [US4] Implementar agregação de 1 min + buffer em RAM + flush em lote no LittleFS (baldes por dia) em `firmware/src/persistencia/medicao.rs`
- [ ] T045 [US4] Adaptar `firmware/src/comunicacao/supabase.rs.template` → `supabase.rs`: usar `Authorization: Bearer <JWT por-device>` (do cache de credenciais, renovação via T019, NÃO anon key); insert em lote de `medicoes`/`eventos`, PATCH de `tomadas` (`estado`, `p_medida_atual`, calibração — só colunas do role `device`); registrar `pub mod supabase` **(depende de Phase 3)**
- [ ] T046 [US4] Implementar pull de config por `config_version` (cache em LittleFS) e leitura de `rele_aberto` (override manual, FR-010a) em `firmware/src/comunicacao/supabase.rs` + `firmware/src/persistencia/config.rs`
- [ ] T047 [US4] Implementar purga de retenção (a cada 5d só confirmados; cap rígido 7d; baldes por dia) em `firmware/src/persistencia/retencao.rs`
- [ ] T048 [US4] Calcular `pi_historico` (24h sobre janela `D_i` dias) no edge e publicar em `tomadas.config` via Supabase em `firmware/src/sensores/potencia.rs` (ou módulo de histórico)

**Checkpoint**: Pipeline completo de dados; offline-first e proteção de flash.

---

## Phase 8: Polish & Cross-Cutting Concerns

**Purpose**: Integração final, dashboard e validação.

- [ ] T049 [P] Integrar o loop principal `medir → decidir → atuar → broadcast → sincronizar (Bearer JWT)` em `firmware/src/main.rs`
- [ ] T050 [P] Adaptar/implementar o dashboard em `dashboard/src/`: login do dono (Supabase Auth), geração de `claim_code` p/ provisionar tomadas, estado/consumo, histórico de `medicoes`, `eventos`/penalidades, toggle `rele_aberto`, config de schedule/criticidade
- [ ] T051 Executar a validação ponta-a-ponta do `quickstart.md` mapeada a SC-001..SC-009
- [ ] T052 Verificar restrições de build (serde_json `opt-level=0`, minilp `opt-level=0`, stack 40 KB, binário cabe na partição) e smoke test on-device

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: sem dependências; T001/T002/T006 furam os riscos de build cedo
- **Foundational (Phase 2)**: depende do Setup — BLOQUEIA tudo abaixo
- **Provisionamento/Auth (Phase 3)**: depende de T012/T014 — BLOQUEIA toda escrita/leitura
  real no Supabase (US2 T035, US4 T045/T046/T048) e o teste multi-nó de US3; **não**
  bloqueia o solver host-only (US1)
- **US1 (Phase 4)**: depende da Foundational — MVP (host, independe de auth/nuvem)
- **US2 (Phase 5)**: depende de US1 (estende o mesmo `solver.rs`); T035 depende de Phase 3
- **US3 (Phase 6)**: depende de US1 (usa `resolver`/`ciclo.rs`) e de Phase 3 (devices
  provisionados); melhor após US2
- **US4 (Phase 7)**: depende da Foundational e da Phase 3 (cliente autenticado); integra
  com US1/US3 no ciclo
- **Polish (Phase 8)**: depende das fases desejadas

### User Story Dependencies

- US1 (P1): independente após Foundational — testável no host, sem nuvem
- US2 (P2): estende o solver de US1; logging na nuvem requer Phase 3
- US3 (P2): consome o solver de US1; requer devices provisionados (Phase 3)
- US4 (P3): paralela a US3 (sensores/persistência), requer Phase 3 p/ a nuvem

### Within Each User Story

- Testes (US1/US2) escritos e falhando antes da implementação
- Tipos antes do solver; solver antes do ciclo; ciclo antes da integração

---

## Parallel Opportunities

- **Setup**: T005, T007 [P]; mas T001→T002→T006 são sequenciais (config → build → spike)
- **Foundational**: T008, T009, T010, T011, T013 [P] (arquivos distintos); T012/T014 isolados
- **Provisionamento**: T016, T017 [P] (arquivos distintos); T015 (Edge Function) em paralelo ao firmware; T018→T019 sequenciais; T020 por último
- **US1 testes**: T021, T022 [P]; **US1 impl**: T026 [P] (atuador) paralelo ao solver
- **US2 testes**: T028–T031 [P]
- **US4 sensores**: T041, T042 [P]
- **Polish**: T049, T050 [P]

### Parallel Example: Foundational + Provisioning

```text
# Foundational (arquivos diferentes, simultâneos):
T008 tipos.rs (solver)    T009 modelo.rs (solver)
T010 config.rs (firmware) T011 medicao.rs (firmware) T013 config_nuvem.rs
# Provisioning (após T012/T014):
T015 functions/provisionar (Edge)  T016 provisionamento.rs (MAC)  T017 credenciais.rs
```

---

## Implementation Strategy

### MVP First (User Story 1)

1. Phase 1 (Setup, **incluindo o spike do `minilp` T006**) → Phase 2 (Foundational) → Phase 4 (US1)
2. **PARAR e VALIDAR**: `cargo test -p tomada-solver` verde; solver decide sob `P_max`
3. Demonstrar o núcleo do TCC (otimização combinatorial) já no host — sem precisar de
   nuvem nem provisionamento

### Incremental Delivery

1. Setup + Foundational → base pronta (riscos de build furados)
2. US1 → solver MVP (host) → validar
3. **Provisionamento/Auth (Phase 3)** → segurança pronta antes de tocar a nuvem (Princípio VIII)
4. US2 → modelo completo (R3/R4/R5 + conforto) + logging seguro → validar
5. US3 → coordenação multi-nó ESP-NOW (devices provisionados) → validar com ≥2 nós
6. US4 → sensores + nuvem (Bearer JWT) + retenção → validar pipeline
7. Polish → dashboard + validação ponta-a-ponta

---

## Notes

- [P] = arquivos diferentes, sem dependência pendente
- O modelo (tópicos 1–4 do PDF) NÃO deve ser alterado (Constituição, Princípio I)
- Manter integralidade binária e factibilidade sempre (folgas)
- Sem gravação em flash por minuto — buffer em RAM + lote (Princípio III)
- **Segurança (Princípio VIII)**: nenhuma credencial por-dispositivo no `.bin`; auth por
  JWT por-device; RLS default-deny; segredo de assinatura só na Edge Function
- Plano B do `minilp`: se o spike T006 falhar, B&B sobre enumeração própria (research §1)
- Commit após cada tarefa ou grupo lógico
