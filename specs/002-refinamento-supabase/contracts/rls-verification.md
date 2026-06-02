# Contract — Verificação de Isolamento RLS (ponta-a-ponta)

Feature: 002-refinamento-supabase | Constituição **VIII**. Valida que a RLS default-deny
do `supabase-schema.sql` (001) é **exercida pelo caminho real do cliente** (HTTP +
`Authorization: Bearer <jwt device>`), não só configurada no banco. Cobre SC-003/004 e
FR-006/007/008/009.

## Pré-condições (setup de teste)

- Schema da 001 aplicado (idempotente) no projeto Supabase de **teste**.
- Duas residências de teste: **A** e **B**, cada uma com ≥1 tomada provisionada.
- JWTs de device para `A.t1` e `B.t1` (via Edge Function `provisionar` com claim-codes de
  teste, ou assinados com o JWT secret do projeto de teste). **Nunca** usar segredos de
  produção.
- Um cliente `anon` (sem Authorization) para os testes negativos.

> Notação: `JWT(A.t1)` = token com claims `tomada_id=A.t1`, `residencia_id=A`.

## Matriz de verificação

| # | Ator (JWT) | Operação | Alvo | Esperado |
|---|-----------|----------|------|----------|
| 1 | `A.t1` | `GET tomadas?id=eq.A.t1` | própria | ✅ 200, 1 linha |
| 2 | `A.t1` | `GET tomadas?residencia_id=eq.A` | irmãs (mesma resid.) | ✅ 200, ≥1 linha |
| 3 | `A.t1` | `GET tomadas?residencia_id=eq.B` | outra residência | ✅ 200 **vazio** (RLS filtra) |
| 4 | `A.t1` | `GET tomadas?id=eq.B.t1` | tomada de B | ✅ 200 **vazio** |
| 5 | `A.t1` | `PATCH tomadas?id=eq.A.t1 {estado}` | própria, coluna permitida | ✅ 2xx |
| 6 | `A.t1` | `PATCH tomadas?id=eq.A.t1 {rele_aberto}` | própria, coluna reservada | ❌ negado (grant de coluna) |
| 7 | `A.t1` | `PATCH tomadas?id=eq.A.t1 {p_max}` | própria, coluna reservada | ❌ negado |
| 8 | `A.t1` | `PATCH tomadas?id=eq.B.t1 {estado}` | linha de B | ❌ negado/0 linhas (RLS `with check`) |
| 9 | `A.t1` | `POST medicoes {tomada_id:A.t1}` | própria | ✅ 2xx |
| 10 | `A.t1` | `POST medicoes {tomada_id:B.t1}` | de B | ❌ negado (`with check`) |
| 11 | `A.t1` | `POST medicoes` (reenvio do #9) | duplicata natural | ✅ 2xx sem duplicar (`ignore-duplicates`) |
| 12 | `A.t1` | `GET medicoes` da residência B | leitura cruzada | ✅ 200 **vazio** |
| 13 | `A.t1` | `GET device_credentials` | tabela de segredo | ❌ negado (sem policy p/ device) |
| 14 | `A.t1` | `GET provisioning_claims` | claims | ❌ negado |
| 15 | `anon` | qualquer `GET/POST` nas tabelas de dados | — | ❌ negado |

> “negado” = `401/403` **ou** `200`/`204` afetando **0 linhas** (PostgREST + RLS pode
> retornar sucesso vazio para filtros que não casam policy). O critério de aprovação é
> **nenhum dado de outro inquilino lido/alterado** e **nenhuma coluna reservada escrita**.

## Execução

Script de host `scripts/verify-rls.sh` (e `.ps1`): roda os 15 casos via `curl`/HTTP,
imprime PASS/FAIL por caso e falha o processo se qualquer linha divergir do esperado.
Documentado no `quickstart.md`. Pode rodar localmente e em CI contra um projeto de teste.

## Critério de aprovação (gate da feature)

- Casos 1, 2, 5, 9, 11 → **permitidos** (operações legítimas).
- Casos 3, 4, 6, 7, 8, 10, 12, 13, 14, 15 → **negados/vazios**.
- 100% conforme ⇒ SC-003 e SC-004 satisfeitos.
