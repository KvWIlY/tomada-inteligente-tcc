# Phase 1 — Data Model

Feature: Refinamento da Implementação do Supabase
Date: 2026-05-30 | Branch: `002-refinamento-supabase`

Este refinamento **não introduz tabelas novas** nem altera o modelo de domínio da 001.
Ele consolida o uso correto das entidades já definidas em
`specs/001-otimizacao-tomadas-milp/data-model.md` e `contracts/supabase-schema.sql`, e
formaliza o **estado local de sincronização** que sustenta a confiabilidade (lote +
confirmação + idempotência + retenção). Abaixo, apenas os deltas relevantes ao escopo.

---

## 1. Credencial Local do Dispositivo (`CredenciaisLocais`) — uso, não redefinição

Já especificada na 001 (§2bis). Aqui ela passa a ser a **única** origem de auth do cliente
de nuvem (substitui as constantes de `config_nuvem.rs`).

| Campo | Tipo | Origem | Uso neste refinamento |
|-------|------|--------|------------------------|
| `tomada_id` | uuid/string | Edge Function | identifica linhas próprias; claim do JWT |
| `residencia_id` | uuid/string | Edge Function | escopo de leitura (irmãs); claim do JWT |
| `device_secret` | string (≥32 B) | `provisionar` | entrada de `renovar` |
| `jwt` | string | `provisionar`/`renovar` | header `Authorization: Bearer` |
| `jwt_exp` | int64 | JWT | decide renovação por **margem** |

**Regras**: lida de LittleFS (`persistencia::credenciais`), **nunca** de compile-time
(FR-002/004). Cache do JWT regravado em lote junto com o resto sempre que possível
(Princípio III).

**Transição de validade do JWT**:
`válido` → (`agora > jwt_exp − margem`) → `renovar()` → `válido`
`válido` → (resposta `401/403`) → `renovar()` + 1 retry → `válido` | `falha → offline+evento`

---

## 2. `config_nuvem` (bootstrap) — delta

| Antes (deriva) | Depois (refinamento) |
|----------------|----------------------|
| `SUPABASE_URL` | `SUPABASE_URL` ✅ mantém |
| `SUPABASE_ANON_KEY` | ❌ **removido** (Princípio VIII) |
| `TOMADA_ID` | ❌ **removido** → vem de `CredenciaisLocais` |
| `RESIDENCIA_ID` | ❌ **removido** → vem de `CredenciaisLocais` |
| Wi-Fi bootstrap | mantém (necessário ao 1º acesso) |

Atualizar também `firmware/.env.example` para refletir a superfície mínima.

---

## 3. Registro Sincronizável local (`Medicao` / evento) — delta de estado

Buffer em LittleFS (baldes por dia). O campo que habilita lote+retenção:

| Campo | Tipo | Notas |
|-------|------|-------|
| `tomada_id` | string | parte da identidade natural |
| `timestamp` | int64 (epoch) | parte da identidade natural; **ordena** o lote |
| `p_medida` / `estado` / `rele_aberto` | — | payload (inalterado da 001) |
| `confirmado` | bool (local) | `false` ao gravar; `true` em lote após 2xx |

**Identidade natural / idempotência**: `(tomada_id, timestamp)` — corresponde ao
`unique (tomada_id, timestamp)` de `medicoes` no schema da 001. Reenvio usa
`Prefer: resolution=ignore-duplicates`.

**Máquina de estados de um registro**:
`gravado(confirmado=false)` → `enviado em lote` → (2xx) → `confirmado=true` → (purga 5 d) → `removido`
`gravado(confirmado=false)` → (sem nuvem) → permanece → (idade > 7 d) → `removido (cap rígido)`

**Validações**: `p_medida ≥ 0`; `timestamp` monotônico por tomada; nunca purgar
`confirmado=false` com idade ≤ 7 d.

---

## 4. Configuração Versionada (`Tomada.config` + `config_version`) — regra de pull

| Campo | Papel no pull |
|-------|----------------|
| `config_version` (remota) | adota a `config` remota **sse** `> config_version` local |
| `config` (jsonb) | parâmetros do modelo (schedule[24], pi_historico[24], w_i, p_max, t_limite, is_critical, p_limiar_detectado_auto) |
| `rele_aberto` (remota) | comando manual; aplicado pelo ciclo, **subordinado a `P_max`** |

**Regra (FR-015)**: versão remota ≤ local ⇒ **no-op** (sem regressão). Versão remota >
local ⇒ adota e grava `config_version = remota`.

**Escrita do device (menor privilégio)**: pode escrever `estado`, `cstd`, `con`,
`p_medida_atual`, `ultimo_uso_ativo_ts`, `config`, `config_version`; **não** pode escrever
`rele_aberto`, `p_max`, `mac`, `residencia_id` (grant de coluna + RLS da 001).

---

## 5. Rastreabilidade requisito → entidade/coluna

| Requisito (spec) | Onde se materializa |
|------------------|---------------------|
| FR-001/004 (JWT, sem anon) | `CredenciaisLocais.jwt`; header Bearer |
| FR-002 (`.bin` genérico) | delta de `config_nuvem` (§2) |
| FR-003 (renovação) | transição de validade do JWT (§1) |
| FR-005..009 (schema/RLS/isolamento) | schema 001 + `contracts/rls-verification.md` |
| FR-010/011 (lote + confirmação) | `confirmado` (§3) |
| FR-012/013 (offline/idempotência) | identidade natural `(tomada_id,timestamp)` (§3) |
| FR-014 (retenção) | máquina de estados do registro (§3) |
| FR-015 (pull por versão) | `config_version` (§4) |
| FR-016 (comando manual) | `rele_aberto` + ciclo subordinado a `P_max` (§4) |
| FR-017 (observabilidade) | `eventos` (tipos de falha de sync) |
