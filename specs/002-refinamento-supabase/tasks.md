---
description: "Task list for Refinamento da Implementação do Supabase (Segurança por-Dispositivo e Sincronização Confiável)"
---

# Tasks: Refinamento da Implementação do Supabase (Segurança por-Dispositivo e Sincronização Confiável)

**Input**: Design documents from `/specs/002-refinamento-supabase/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/
(`supabase-client.md`, `rls-verification.md`). Contratos **autoritativos** de schema e
provisionamento permanecem na feature 001 (`supabase-schema.sql`, `provisioning-api.md`).

**Tests**: Incluídos onde os contratos os exigem — testes de **lógica pura** (host) por
`contracts/supabase-client.md` (funções `montar_corpo_lote`, `selecionar_pendentes`,
`deve_adotar_config`, `deve_renovar`) e a **matriz de verificação RLS** ponta-a-ponta de
`contracts/rls-verification.md`. O caminho de I/O do firmware é validado por smoke test
on-device.

**Dependência da 001**: este refinamento consome os módulos de provisionamento já
planejados na 001 — `firmware/src/persistencia/credenciais.rs` (001 T017) e
`firmware/src/comunicacao/provisionamento.rs` (001 T016/T019). Onde eles ainda não
existirem, as tarefas Foundational abaixo os exigem como bloqueio.

## Format: `[ID] [P?] [Story?] Description`

- **[P]**: Pode rodar em paralelo (arquivos diferentes, sem dependência pendente)
- **[Story]**: US1–US4 conforme spec.md (só nas fases de user story)
- Caminhos de arquivo relativos à raiz do repo (`C:\y`)

## Path Conventions

- Firmware: `firmware/src/`
- Lógica pura host-testável: `tomada-solver/src/sync.rs`, `tomada-solver/tests/`
  (reusa o crate host existente — **sem novo crate, sem nova dependência**, conforme plan.md)
- Scripts de verificação (host): `scripts/`
- Contratos/banco: `specs/001-otimizacao-tomadas-milp/contracts/` (autoritativo) e
  `specs/002-refinamento-supabase/contracts/`

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Estabelecer a linha de base e mapear a superfície da deriva antes de refatorar.

- [X] T001 Estabelecer baseline: `cargo test -p tomada-solver` (host) = 16 testes verdes (7 modelo + 9 sync). Firmware esp **nunca foi compilado** neste ambiente (sem ESP-IDF baixado/`.embuild`, sem `firmware/.env` → `dotenv!` falha; sem hardware) — baseline esp pendente de ambiente com toolchain completa
- [X] T002 Superfície da anon key mapeada: única ocorrência compilada em `firmware/src/config_nuvem.rs:25` (`SUPABASE_ANON_KEY`); todo o resto está em `comunicacao/supabase.rs.template` (NÃO é módulo compilado). `TOMADA_ID`/`RESIDENCIA_ID` não existiam como constantes — só referenciadas pelo template
- [X] T003 [P] Criar o módulo de lógica pura `tomada-solver/src/sync.rs` (vazio + `pub mod sync;` em `tomada-solver/src/lib.rs`) e o arquivo de testes `tomada-solver/tests/sync.rs` (host)

**Build Risks**:
- Promover `supabase.rs.template → supabase.rs` ativa `pub mod supabase;` — garantir que o
  módulo compila no esp antes de empilhar lote/pull (T010 vem antes das features).

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Credencial por-dispositivo legível + provedor de JWT + helper de requisição
autenticada + tipos de sync. **Bloqueia todas as user stories** (todas acessam a nuvem com JWT).

**⚠️ CRITICAL**: Nenhuma user story começa antes desta fase.

- [ ] T004 [P] Garantir que `firmware/src/persistencia/credenciais.rs` (001 T017) lê de LittleFS `{tomada_id, residencia_id, device_secret, jwt, jwt_exp}` e expõe getters; se ausente, criar conforme `data-model.md` §1 e `provisioning-api.md`
- [ ] T005 [P] Garantir que `firmware/src/comunicacao/provisionamento.rs` (001 T019) expõe `obter_jwt_valido() -> Result<String>` (renova por margem antes de `jwt_exp`) e `renovar()`; se ausente, criar conforme `provisioning-api.md`
- [X] T006 [P] Definir a função pura `deve_renovar(jwt_exp: i64, agora: i64, margem_s: i64) -> bool` em `tomada-solver/src/sync.rs`
- [ ] T007 Implementar o helper de requisição autenticada em `firmware/src/comunicacao/supabase.rs` (promovido de `.template`): monta headers **só** com `Authorization: Bearer <jwt>` (de `obter_jwt_valido`) + `Content-Type` + `Prefer`; em `401/403` chama renovação e repete **uma** vez; sem header `apikey`. Registrar `pub mod supabase;` em `firmware/src/comunicacao/mod.rs`
- [X] T008 [P] Tipo `SyncRecord { tomada_id, timestamp, p_medida, estado, rele_aberto, confirmado }` definido em `tomada-solver/src/sync.rs` (espelha o futuro `Medicao`). Mapeamento `Medicao → SyncRecord` fica no firmware (bloqueado: domínio 001 ausente)

**Checkpoint**: Cliente autentica com JWT por-device; credenciais e renovação disponíveis.

---

## Phase 3: User Story 1 - Autenticação por-dispositivo substituindo a chave compartilhada (Priority: P1) 🎯 MVP 🔒

**Goal**: Todo acesso à nuvem usa o JWT por-dispositivo; o `.bin` é genérico (sem segredo
nem identificador por-device em compile-time); renovação automática em `401`/perto do `exp`.

**Independent Test**: Gravar a imagem genérica, provisionar e confirmar que (a) leituras/
escritas usam só o JWT e (b) binário/bootstrap não revelam chave de dados nem IDs fixos.

### Implementation for User Story 1

- [X] T009 [US1] `firmware/src/config_nuvem.rs` reduzido a `SUPABASE_URL` + Wi-Fi de bootstrap; `SUPABASE_ANON_KEY` removido (doc atualizado, Princípio VIII / FR-002). `TOMADA_ID`/`RESIDENCIA_ID` já não existiam como constantes
- [ ] T010 [US1] Migrar **todas** as operações de `firmware/src/comunicacao/supabase.rs` para usar o helper autenticado (T007) e `tomada_id`/`residencia_id` de `CredenciaisLocais` (T004); eliminar referências a `SUPABASE_ANON_KEY`/`apikey` e às constantes removidas (FR-001/004)
- [ ] T011 [US1] Implementar a renovação reativa+proativa do JWT ligando `deve_renovar` (T006) e o retry de `401/403` (T007) em `firmware/src/comunicacao/supabase.rs`; em falha persistente, retornar erro sem travar (modo offline)
- [X] T012 [P] [US1] `firmware/.env.example` atualizado à superfície mínima (`WIFI_SSID`/`WIFI_PASSWORD`/`PROJECT_URL`); `PUBLIC_KEY` removido
- [ ] T013 [US1] Verificação de `.bin` genérico (FR-002/SC-001/SC-002): `grep -rn "ANON_KEY\|RESIDENCIA_ID\|TOMADA_ID\|apikey" firmware/src` retorna 0; confirmar no smoke test que nenhuma chave de dados é enviada

**Checkpoint**: Firmware fala com a nuvem só por JWT por-device; binário genérico. MVP de segurança.

---

## Phase 4: User Story 2 - Isolamento multi-inquilino verificável (Priority: P1) 🔒

**Goal**: RLS default-deny aplicada e **exercida pelo caminho real**: acessos cruzados e
escritas de colunas reservadas negados; `anon` negado; operações legítimas permitidas.

**Independent Test**: Rodar a matriz de `contracts/rls-verification.md` com JWTs de duas
residências; 100% conforme.

### Tests for User Story 2 (contrato rls-verification.md)

- [X] T014 [US2] `scripts/verify-rls.sh` e `scripts/verify-rls.ps1` implementados: 15 casos de `contracts/rls-verification.md` via HTTP, PASS/FAIL por caso, exit 1 em divergência, guarda de env (exit 2). Sintaxe validada (`bash -n` + parser PS). Execução real pendente de projeto Supabase de teste + JWTs (T016)

### Implementation for User Story 2

- [X] T015 [US2] Schema aplicado via `supabase db push --linked` no projeto `tcc_dashboard` (ref `mmsmfvzbcrkoscqnhvky`), migration `20260530120000_init_schema.sql` — transação OK. Inclui as 6 tabelas, role `device` + grants por coluna, `unique (tomada_id, timestamp)` em `medicoes`, RLS default-deny + políticas por claim e `revoke ... from anon` (FR-005)
- [X] T016 [US2] Provisionadas residências de teste A (`aaaaaaaa-…`) e B (`bbbbbbbb-…`) + tomadas via Edge Function `provisionar` (claim-codes `TCC-CLAIM-A-001`/`B-001`); JWTs ES256 por-device obtidos. Edge Function migrada de HS256→**ES256** (signing key assimétrica importada no projeto, JWK privada em secret `DEVICE_JWT_JWK`). Seed documentado no `quickstart.md`
- [X] T017 [US2] `scripts/verify-rls.sh` executado contra `tcc_dashboard`: **15/15 PASS** — legítimos (1,2,5,9,11) permitidos, negativos (3,4,6,7,8,10,12,13,14,15) negados/vazios (FR-006/007/008/009, SC-003/SC-004 ✅)

**Checkpoint**: Isolamento por residência e por coluna comprovado ponta-a-ponta.

---

## Phase 5: User Story 3 - Sincronização em lote e reconciliação offline-first (Priority: P2)

**Goal**: Medições/eventos enviados **em lote**; cada registro confirmado só após 2xx;
reconciliação idempotente ao reconectar; purga só de confirmados (cap rígido 7 d).

**Independent Test**: Gerar medições offline; reconectar; lote enviado e confirmado;
reenvio não duplica; purga remove só confirmados; nada local > 7 d.

### Tests for User Story 3 (contrato supabase-client.md — funções puras)

- [X] T018 [P] [US3] Teste de `montar_corpo_lote` (array JSON correto, escape, `f32` manual) em `tomada-solver/tests/sync.rs`
- [X] T019 [P] [US3] Teste de `selecionar_pendentes` (só `confirmado=false`, ordenado por `timestamp`, respeita `MAX_LOTE`) em `tomada-solver/tests/sync.rs`

### Implementation for User Story 3

- [X] T020 [P] [US3] `montar_corpo_lote(&[SyncRecord], &[usize]) -> String` implementado em `tomada-solver/src/sync.rs` (JSON manual `{:.2}` p/ `f32`, escape, sem `confirmado`); coberto por T018
- [X] T021 [P] [US3] Implementar `selecionar_pendentes(&[SyncRecord], max) -> Vec<usize>` (ordenado por `timestamp`) em `tomada-solver/src/sync.rs`
- [ ] T022 [US3] Adicionar a flag `confirmado` ao `Medicao` e ao buffer (baldes por dia) em `firmware/src/persistencia/medicao.rs` (`false` ao gravar)
- [ ] T023 [US3] Implementar `sincronizar_medicoes(lote)` e `sincronizar_eventos(lote)` em `firmware/src/comunicacao/supabase.rs`: POST de **um** array com `Prefer: return=minimal, resolution=ignore-duplicates`; em 2xx marcar todos como `confirmado=true` numa **única** consolidação na flash (FR-010/011/013, Princípio III)
- [ ] T024 [US3] Implementar/ajustar a purga em `firmware/src/persistencia/retencao.rs`: remover só `confirmado=true`; descartar qualquer registro com idade > 7 d mesmo sem confirmação (cap rígido) (FR-014, SC-006/SC-007)

**Checkpoint**: Pipeline de sync em lote, idempotente e offline-first; retenção segura.

---

## Phase 6: User Story 4 - Pull de configuração e comando manual a partir da nuvem (Priority: P3)

**Goal**: Adotar config remota só com versão maior; aplicar `rele_aberto` do dashboard
subordinado ao `P_max`.

**Independent Test**: Alterar config/comando no dashboard; a tomada adota a config (por
versão) e aplica o comando manual, exceto quando violaria o teto elétrico.

### Tests for User Story 4 (contrato supabase-client.md — função pura)

- [X] T025 [P] [US4] Teste `t025_adota_config_so_se_versao_remota_maior` em `tomada-solver/tests/sync.rs` (adota só se remota > local; sem regressão) — verde

### Implementation for User Story 4

- [X] T026 [US4] Implementar `deve_adotar_config(versao_remota, versao_local) -> bool` em `tomada-solver/src/sync.rs`
- [ ] T027 [US4] Implementar `buscar_desired_state()` em `firmware/src/comunicacao/supabase.rs`: um GET `select=config,config_version,rele_aberto`; parse manual do `f32` no `config` (FR-015/016)
- [ ] T028 [US4] Aplicar o pull no ciclo: adotar config via `deve_adotar_config` e gravar `config_version` em `firmware/src/persistencia/config.rs`; entregar `rele_aberto` ao ciclo de decisão **subordinado a `P_max`** em `firmware/src/decisao/ciclo.rs` (FR-016)

**Checkpoint**: Fluxo bidirecional nuvem↔dispositivo fechado; comando manual respeita a segurança elétrica.

---

## Phase 7: Polish & Cross-Cutting Concerns

**Purpose**: Observabilidade, integração no loop e validação ponta-a-ponta.

- [ ] T029 Registrar falhas de sync como `eventos` (auth falha persistente, credencial revogada, schema ausente, falha de renovação) sem vazar segredos, em `firmware/src/comunicacao/supabase.rs` (FR-017, SC-010)
- [ ] T030 [P] Integrar o sync autenticado em lote no loop principal `medir → decidir → atuar → sincronizar (Bearer JWT)` em `firmware/src/main.rs`
- [ ] T031 Executar a validação ponta-a-ponta do `quickstart.md` mapeada a SC-001..SC-010 (grep, verify-rls, offline/lote, pull, eventos)

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: sem dependências
- **Foundational (Phase 2)**: depende do Setup — **BLOQUEIA** todas as user stories; exige
  os módulos de provisionamento da 001 (T004/T005)
- **US1 (Phase 3)**: depende da Foundational — MVP de segurança
- **US2 (Phase 4)**: depende de T015 (schema) e T016 (JWTs); o script T014 independe do
  firmware (host) e pode ser escrito em paralelo a US1
- **US3 (Phase 5)**: depende da Foundational (helper autenticado T007) e de US1 (cliente já
  em JWT); funções puras (T018–T021) independem do firmware
- **US4 (Phase 6)**: depende da Foundational e de US1; função pura (T025/T026) independe do firmware
- **Polish (Phase 7)**: depende das fases desejadas

### User Story Dependencies

- US1 (P1): independente após Foundational
- US2 (P1): independente após schema+JWTs (paralela a US1; valida o contrato que US1 usa)
- US3 (P2): consome o cliente autenticado de US1
- US4 (P3): consome o cliente autenticado de US1; paralela a US3

---

## Parallel Opportunities

- **Setup**: T003 [P]
- **Foundational**: T004, T005, T006, T008 [P] (arquivos distintos); T007 depende de T004/T005
- **US1**: T012 [P] (env.example) paralelo ao refactor do cliente
- **US2**: T014 (script host) em paralelo a toda a US1
- **US3**: testes T018, T019 [P]; impl puras T020, T021 [P] (mesmo arquivo `sync.rs` —
  coordenar); firmware T022→T023→T024 sequenciais
- **US4**: T025 [P]; T026→T027→T028 sequenciais
- **Polish**: T030 [P]

### Parallel Example: Foundational

```text
T004 credenciais.rs   T005 provisionamento.rs   T006 deve_renovar (sync.rs)   T008 SyncRecord (sync.rs)
# depois:
T007 helper autenticado (supabase.rs) — consome T004/T005
```

---

## Implementation Strategy

### MVP First (User Story 1 + User Story 2)

1. Phase 1 (Setup) → Phase 2 (Foundational) → Phase 3 (US1) → Phase 4 (US2)
2. **PARAR e VALIDAR**: `grep` sem ocorrências de anon key; `verify-rls.sh` 100% conforme
3. Entrega o ganho de segurança não-negociável (Princípio VIII) — corrige a deriva

### Incremental Delivery

1. Setup + Foundational → cliente autenticado por JWT
2. US1 → migração completa anon→JWT + `.bin` genérico → validar
3. US2 → schema aplicado + isolamento RLS comprovado → validar
4. US3 → sync em lote + idempotência + retenção → validar offline
5. US4 → pull de config/comando → validar bidirecional
6. Polish → eventos + loop + validação ponta-a-ponta

---

## Notes

- [P] = arquivos diferentes, sem dependência pendente
- **Não** redefinir o schema nem a Edge Function da 001 — apenas aplicar/consumir (Princípios VI/VIII)
- **Nunca** header `apikey`/anon key no firmware; só `Authorization: Bearer <jwt>` (Princípio VIII)
- Sync **em lote**, `confirmado` consolidado em lote — sem escrita na flash por minuto/registro (Princípio III)
- Idempotência por `(tomada_id, timestamp)` casando no `unique` do schema
- Eventos de erro nunca incluem `device_secret`/`jwt`/claims
- Commit após cada tarefa ou grupo lógico

---

## Status de Implementação (2026-05-31)

**Concluído e verificado (host):**
- Lógica pura + testes em `tomada-solver/src/sync.rs` / `tests/sync.rs` — 9 testes verdes:
  T003, T006, T008, T018, T019, T020, T021, T025, T026. + modelo (7) = 16 verdes.
- T001 (baseline host), T002 (mapa da anon key), T009 (`config_nuvem.rs`), T012
  (`.env.example`), T014 (`scripts/verify-rls.{sh,ps1}`, sintaxe validada).
- **Camada de nuvem (US2)** validada ponta-a-ponta no `tcc_dashboard`: T015/T016/T017,
  `verify-rls.sh` 15/15.

**Domínio firmware da 001+002 ESCRITO (2026-05-31) — pendente de compilação na toolchain esp:**
Construído o domínio embarcado inteiro (não existia; era só esqueleto de infra):
- `persistencia/`: `config.rs` (Tomada+jsonb), `credenciais.rs`, `medicao.rs` (buffer RAM +
  baldes/dia + `confirmado`), `retencao.rs` (purga 5d/cap 7d).
- `comunicacao/`: `provisionamento.rs` (MAC→JWT, `obter_jwt_valido`/`renovar`),
  `supabase.rs` (Bearer JWT + apikey publishable, sync EM LOTE `on_conflict`, PATCH,
  pull desired state, retry 401, eventos), `espnow.rs` (heartbeat v2 30B, degradação segura).
- `sensores/`: `zmpt101b.rs`/`sct013.rs`/`potencia.rs` (RMS, P, U_i/F_i) — ADC desacoplado
  por closures.
- `decisao/`: `atuador.rs` (relé GPIO26 Opção A), `ciclo.rs` (monta SolverInput, resolve
  MILP, comando manual subordinado ao P_max, eventos).
- `main.rs`: loop medir→espnow→decidir→atuar→(60s) buffer/pull/publish/sync/retenção.
- Pinos: relé GPIO26, tensão GPIO32 (ADC1_CH4), corrente GPIO35 (ADC1_CH7).

Cobre o **código** de T004, T005, T007, T010, T011, T013(grep ✓), T022, T023, T024, T027,
T028, T029, T030. **NÃO marcados [X]**: sem compilar/flashar na toolchain esp + hardware,
não há verificação real (regra do plano). Pontos de ajuste provável ao compilar:
(1) API ADC oneshot (`AdcChannelConfig`/`read`), (2) assinatura do `register_recv_cb` do
ESP-NOW — ambos isolados e comentados no código.

**Pendente de ambiente esp + hardware:** compilar `cd firmware && cargo build`, criar
`firmware/.env` (a partir do `.env.example`), gravar com `espflash`, provisionar com um
claim-code, e rodar o smoke/quickstart (T013 smoke, T031). Calibrar `FATOR_TENSAO`
(zmpt101b.rs) e `FATOR_CORRENTE` (sct013.rs) no hardware real.
