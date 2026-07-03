# Implementation Plan: Refinamento da Implementação do Supabase (Segurança por-Dispositivo e Sincronização Confiável)

**Branch**: `002-refinamento-supabase` | **Date**: 2026-05-30 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/002-refinamento-supabase/spec.md`

## Summary

Fazer a **implementação real do cliente Supabase no firmware** convergir para o contrato
de nuvem já especificado na feature 001 (`contracts/supabase-schema.sql`,
`contracts/provisioning-api.md`) e para o Princípio VIII da constituição. Hoje há uma
deriva: `firmware/src/comunicacao/supabase.rs.template` autentica com **anon key
compartilhada** + `TOMADA_ID`/`RESIDENCIA_ID` lidos de `config_nuvem.rs` (credencial
por-dispositivo no binário), e a sincronização é uma escrita unitária por chamada, sem
lote nem handshake de confirmação. O refinamento: (1) substitui a anon key por **JWT
por-dispositivo** lido do cache de credenciais (com renovação em `401`/perto do `exp`);
(2) torna o `.bin` genérico (remove segredos/identificadores de compile-time);
(3) aplica e **valida a RLS default-deny** ponta-a-ponta; (4) implementa **sync em lote
+ confirmação + reconciliação idempotente** sustentando a purga 5 d/cap 7 d; (5) liga o
**pull** de config versionada e do comando manual de relé.

Não altera o modelo MILP, a Edge Function de provisionamento, sensores, atuação nem a
coordenação ESP-NOW. É um refinamento da **camada de nuvem**, reaproveitando os módulos
e contratos já existentes da 001.

## Technical Context

**Language/Version**: Rust edition 2021, `rust-version = 1.82`; toolchain `esp` (Xtensa)
para o firmware; toolchain stable do host para testes de lógica pura (parsing/idempotência).

**Primary Dependencies**: `esp-idf-svc 0.52.1` (HTTP client com cert bundle global),
`embedded-svc 0.29`, `serde`/`serde_json` (`opt-level = 0` — bug LLVM Xtensa em `f32`;
JSON manual onde houver `f32`), `anyhow`. **Sem novas dependências** — o refinamento usa o
que a 001 já trouxe. A credencial é produzida pela Edge Function existente (`djwt` no lado
Deno, fora do firmware).

**Storage**: LittleFS no ESP32 — credenciais provisionadas (`tomada_id`, `residencia_id`,
`device_secret`, cache `jwt`/`jwt_exp`) via `persistencia::credenciais`; buffer de
medições/eventos com flag `confirmado` em baldes por dia; Supabase (PostgREST) como fonte
única de verdade na nuvem.

**Identidade/Auth**: identidade por MAC → `tomada_id` UUID; autenticação por **JWT
por-dispositivo** (role `device`, claims `residencia_id`/`tomada_id`) no header
`Authorization: Bearer <jwt>` + header `apikey` com a **publishable key pública** (exigida
pelo gateway hospedado, sem acesso sob RLS) — **nunca** anon key nem `service_role`. Renovação via Edge
Function `renovar(tomada_id, device_secret)`. Schema com **RLS default-deny** + menor
privilégio por coluna (`device` não escreve `rele_aberto`/`p_max`/`mac`/`residencia_id`).

**Testing**: `cargo test` no host para a lógica pura extraível (idempotência de lote,
seleção de pendentes, decisão de pull por versão, escape/parse de JSON manual). Validação
de RLS ponta-a-ponta via chamadas reais ao PostgREST (script de verificação, `quickstart`).
Smoke test on-device para o ciclo completo de sync autenticado.

**Target Platform**: ESP32 (Xtensa, esp-idf, std); host Linux/Windows para testes de
lógica e para os scripts de verificação de RLS.

**Project Type**: multi-componente — firmware embarcado (Rust/ESP32) + banco em nuvem
(Supabase: schema + Edge Function já existente) + dashboard (consumidor, fora do escopo).

**Performance Goals**: sync **em lote** (1 chamada por flush, não por minuto/registro);
manter a decisão de estado em ≤10 s (SC-008 da 001) — o sync não bloqueia o ciclo de
decisão. Renovação de JWT antecipada (margem antes do `exp`) para não atrasar o flush.

**Constraints**: proteção de flash (sem escrita por minuto — buffer em RAM + flush em
lote; flag `confirmado` consolidada em lote, Princípio III); `serde_json` em `opt-level=0`;
nenhum segredo/identificador por-dispositivo em compile-time (Princípio VIII);
offline-first (opera e reconcilia ao reconectar); idempotência via identidade natural
`(tomada_id, timestamp)`.

**Scale/Scope**: até ~8 tomadas por residência; medições 1/min; retenção local 7 dias com
purga a cada 5; lote de sync dimensionado para caber na RAM/stack (40 KB main task) e na
resposta HTTP.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

Avaliação contra a Constituição v1.2.0 (foco nos princípios tocados por este refinamento):

| Princípio | Conformidade do plano |
|-----------|------------------------|
| I. Modelagem rigorosa e sempre factível | ✅ N/A direto — o refinamento **não toca** o modelo MILP; preserva-o intacto. |
| II. Complexidade justificada | ✅ **Reduz** complexidade/risco: remove a anon key e unifica a auth em JWT por-device; **zero novas dependências**; extrai lógica pura para testes no host. |
| III. Confiabilidade do hardware | ✅ Sync em lote + flag `confirmado` consolidada em lote; sem escrita na flash por minuto/registro; purga só de confirmados, cap rígido 7 d. |
| IV. Inteligência preditiva | ✅ N/A — `Π_{i,t}` inalterado; o pull de config apenas transporta parâmetros já definidos. |
| V. Coordenação multi-nó | ✅ N/A direto — ESP-NOW fora do escopo; a leitura de tomadas-irmãs (fallback pela nuvem) passa a usar JWT/RLS corretamente, sem mudar a coordenação. |
| VI. Supabase como fonte única de verdade | ✅ Núcleo do refinamento: offline-first, reconciliação **idempotente** por `(tomada_id, timestamp)`, pull de desired state (config/`rele_aberto`) por versão. |
| VII. Observabilidade e reprodutibilidade | ✅ Falhas de sync (401 persistente, credencial revogada, schema ausente, renovação falha) viram `eventos` sem vazar segredos. |
| VIII. Segurança e isolamento multi-inquilino | ✅ **Razão de existir**: JWT por-device em vez de anon key; `.bin` genérico (sem segredo/identificador em compile-time); RLS default-deny validada ponta-a-ponta; menor privilégio por coluna; segredo de assinatura só na Edge Function. |

**Resultado**: PASS. Sem violações. O refinamento elimina uma violação preexistente do
Princípio VIII (anon key compartilhada no binário) e fortalece os Princípios III e VI.
Sem entradas em *Complexity Tracking*.

## Project Structure

### Documentation (this feature)

```text
specs/002-refinamento-supabase/
├── plan.md              # Este arquivo
├── spec.md              # Especificação (US1–US4, FR-001..017, SC-001..010)
├── research.md          # Phase 0 (decisões: auth header, idempotência, lote, pull)
├── data-model.md        # Phase 1 (entidades do refinamento + delta de schema)
├── quickstart.md        # Phase 1 (aplicar schema, deploy fn, verificar RLS, smoke)
├── contracts/
│   ├── supabase-client.md      # Contrato do cliente do firmware (operações + headers JWT)
│   └── rls-verification.md     # Matriz de testes de isolamento RLS ponta-a-ponta
└── checklists/
    └── requirements.md         # Checklist de qualidade da spec (já gerado)
```

> Os contratos **autoritativos** de schema e provisionamento permanecem na 001
> (`specs/001-otimizacao-tomadas-milp/contracts/supabase-schema.sql` e
> `provisioning-api.md`). Os contratos desta feature **referenciam-nos** e descrevem como
> o cliente os consome — não os duplicam nem os redefinem.

### Source Code (repository root)

```text
firmware/
├── src/
│   ├── config_nuvem.rs            # MUDA: só SUPABASE_URL + Wi-Fi bootstrap
│   │                              #   (REMOVER SUPABASE_ANON_KEY/TOMADA_ID/RESIDENCIA_ID)
│   ├── comunicacao/
│   │   ├── mod.rs                 # registra `pub mod supabase;` (deixa de ser .template)
│   │   ├── supabase.rs            # NOVO (de supabase.rs.template): Bearer <JWT>, sem apikey;
│   │   │                          #   sync em lote, idempotência, pull config/rele
│   │   ├── provisionamento.rs     # USA: obter_jwt_valido()/renovar() em 401/perto do exp
│   │   └── wifi_provisioning.rs   # (existente) coleta claim_code — não muda aqui
│   └── persistencia/
│       ├── credenciais.rs         # USA: ler {tomada_id, residencia_id, device_secret, jwt, jwt_exp}
│       ├── medicao.rs             # MUDA: flag `confirmado`; seleção de pendentes p/ lote
│       └── retencao.rs            # USA: purga só `confirmado=true`; cap rígido 7 d

supabase/
├── functions/provisionar/        # EXISTENTE — não reescrever (apenas consumir)
└── (schema aplicado via contracts/supabase-schema.sql da 001 — verificado aqui)

scripts/ (host)
└── verify-rls.sh|.ps1            # NOVO — exercita a matriz de contracts/rls-verification.md
```

**Structure Decision**: Sem novos crates nem novas dependências. O refinamento atua no
crate `firmware` existente, promovendo `supabase.rs.template → supabase.rs` com o modelo
de auth correto e adicionando a semântica de lote/confirmação/pull. A lógica pura
(idempotência, seleção de pendentes, decisão de pull por versão, parse/escape de JSON
manual) é isolada em funções testáveis no host (`cargo test`), satisfazendo o Princípio
VII sem exigir hardware. A verificação de RLS é um script de host que bate no PostgREST
real. Isso mantém o Princípio II (sem complexidade nova) e corrige o Princípio VIII.

## Complexity Tracking

> Sem violações da Constituição que exijam justificativa. O refinamento **remove** uma
> violação (anon key no binário) e não adiciona dependências. Abaixo, **uma decisão
> esclarecida** (não é violação): a publishable key pública no `.bin`.

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| Publishable key pública (`apikey`) embutida no `.bin`, compartilhada entre nós | O gateway hospedado **exige** `apikey` em `/rest/v1/*` (responde `401 "No API key found"` antes da RLS). A publishable key (`sb_publishable_…`) é pública por design. | Omitir o header não é opção (gateway recusa). Reusar a antiga anon key **foi rejeitado**: concedia acesso a dados. A publishable key **não concede acesso** (RLS default-deny, verificado 15/15 em T017) → respeita o Princípio VIII; o único segredo de dados continua sendo o JWT por-device. |
