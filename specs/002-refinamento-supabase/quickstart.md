# Quickstart — Refinamento da Implementação do Supabase

Feature: 002-refinamento-supabase | Date: 2026-05-30

Passos para aplicar, validar e exercitar o refinamento da camada Supabase. Assume o
ambiente da 001 já instalado (toolchain `esp`, Supabase CLI, projeto criado).

## 1. Aplicar o schema (idempotente)

No SQL Editor do Supabase (ou via CLI), aplicar o contrato autoritativo da 001:

```bash
# specs/001-otimizacao-tomadas-milp/contracts/supabase-schema.sql
# Pode rodar mais de uma vez (idempotente): tabelas, role `device`, grants por coluna,
# RLS default-deny e políticas por claim.
```

Confirmar que `medicoes` tem `unique (tomada_id, timestamp)` (pré-requisito da
idempotência) e que a RLS está habilitada em todas as tabelas.

## 2. Edge Function de provisionamento (já existente)

```bash
supabase secrets set PROJECT_JWT_SECRET="<jwt secret do projeto>"
supabase functions deploy provisionar --no-verify-jwt
```

(Sem mudanças nesta feature — apenas garantir que está no ar para emitir/renovar JWTs.)

## 3. Reduzir a superfície de bootstrap do firmware (Princípio VIII)

- Editar `firmware/src/config_nuvem.rs`: manter **só** `SUPABASE_URL` + Wi-Fi de
  bootstrap. **Remover** `SUPABASE_ANON_KEY`, `TOMADA_ID`, `RESIDENCIA_ID`.
- Atualizar `firmware/.env.example` para refletir a superfície mínima.
- Confirmar que nada em compile-time referencia mais a anon key:

```bash
grep -rn "ANON_KEY\|RESIDENCIA_ID\|TOMADA_ID" firmware/src   # esperado: 0 ocorrências
```

## 4. Promover o cliente: `supabase.rs.template` → `supabase.rs`

- Trocar todos os headers para `Authorization: Bearer <jwt>` (sem `apikey`), com o JWT de
  `provisionamento::obter_jwt_valido()`.
- Implementar lote + `confirmado` + `resolution=ignore-duplicates` (ver
  `contracts/supabase-client.md`).
- Implementar `buscar_desired_state()` (config por versão + `rele_aberto`).
- Registrar `pub mod supabase;` em `firmware/src/comunicacao/mod.rs`.

## 5. Testes de lógica pura (host, sem hardware)

```bash
cargo test -p firmware --lib    # ou o alvo onde vivem as funções puras
# cobre: montar_corpo_lote, selecionar_pendentes, deve_adotar_config, deve_renovar
```

## 6. Verificação de RLS ponta-a-ponta

```bash
export SUPABASE_URL=https://<proj-teste>.supabase.co
# JWTs de teste de duas residências (via provisionar com claim-codes de teste)
export JWT_A=...   JWT_B=...
bash scripts/verify-rls.sh      # 15 casos de contracts/rls-verification.md
# PASS em todos ⇒ SC-003/SC-004 satisfeitos
```

## 7. Smoke test on-device (ciclo autenticado)

```bash
cd firmware && cargo run        # requer ESP32 físico
```

Validar no log/dashboard:
- provisionamento → obtém JWT; nenhuma anon key usada;
- flush em **lote** de `medicoes` (não 1/min); registros viram `confirmado`;
- reconexão após offline reconcilia sem duplicar;
- pull adota config só com versão maior; `rele_aberto` respeitado sob `P_max`.

## 8. Mapa de validação → critérios de sucesso

| Passo | Cobre |
|-------|-------|
| 3 (grep) + 4 | SC-001, SC-002 |
| 6 (verify-rls) | SC-003, SC-004 |
| 7 (offline/lote) | SC-005, SC-006, SC-007, SC-008 |
| 7 (pull) | SC-009 |
| 7 + eventos | SC-010 |

## 9. Notas operacionais da integração (tcc_dashboard, 2026-05-31)

Gotchas reais encontrados ao integrar o projeto Supabase via CLI (verify-rls = 15/15):

1. **Habilitar a Data API.** Projetos novos podem vir com a Data API (PostgREST)
   **desligada** → todo `/rest/v1/*` dá `PGRST002`. Ligar em **Project Settings → Data
   API → Enable Data API** e expor o schema `public`.
2. **Grants explícitos (auto-expose OFF).** Desde 2026-05-30 o default de
   `auto_expose_new_tables` é `false`: tabelas novas **não** recebem grant automático para
   `service_role`/`authenticated`/`device`. Migrations: `…_init_schema.sql` (grants do
   `device`) + `…_grants_data_api_roles.sql` (`service_role` total, `authenticated` por
   tabela). O `device` precisa de **SELECT + INSERT** em `medicoes`/`eventos`.
3. **JWT por-device em ES256.** O projeto usa JWT Signing Keys ECC P-256. Gerar
   `supabase gen signing-key --algorithm ES256`, **importar** a JWK no projeto (vira standby
   no JWKS) e setar a **JWK privada** no secret `DEVICE_JWT_JWK` (use a parte com `"d"`,
   não a pública). Deploy: `supabase functions deploy provisionar --no-verify-jwt`.
4. **Recarregar o cache do PostgREST após grants:** `notify pgrst, 'reload schema';` (ou
   reiniciar o projeto). Sem isso, novos grants podem demorar a valer para escrita.
5. **Auth da CLI no Windows:** `supabase login` pode não persistir o token (Credential
   Manager). Use `$env:SUPABASE_ACCESS_TOKEN = "sbp_…"` na mesma sessão.
6. **Cliente:** toda requisição leva `apikey: <publishable key>` (pública) **+**
   `Authorization: Bearer <jwt device>`. Idempotência de `medicoes`:
   `POST …/medicoes?on_conflict=tomada_id,timestamp` com `resolution=ignore-duplicates`.
