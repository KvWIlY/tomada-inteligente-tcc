-- Grants explícitos para os roles do Data API (feature 002 / integração tcc_dashboard).
--
-- Motivo: a partir de 2026-05-30 o Supabase virou o default de `auto_expose_new_tables`
-- para `false` (ver supabase/config.toml). Tabelas criadas pela migration de schema
-- (20260530120000_init_schema.sql) NÃO recebem mais GRANT automático para `anon`,
-- `authenticated` e `service_role`. O role `device` já recebeu grants explícitos lá;
-- aqui completamos `service_role` (Edge Function de provisionamento) e `authenticated`
-- (dashboard / dono humano). `anon` continua sem nenhum grant (negado por padrão).
--
-- Idempotente: GRANT é repetível sem erro.

-- ============================================================
-- service_role: backend confiável (Edge Function `provisionar`). Faz bypass de RLS,
-- mas ainda precisa de GRANT de tabela quando o auto-expose está desligado.
-- Acessa todas as tabelas, inclusive as de segredo (device_credentials,
-- provisioning_claims) que device/authenticated nunca tocam.
-- ============================================================
grant all privileges on table
  residencias, tomadas, medicoes, eventos, device_credentials, provisioning_claims
  to service_role;

-- ============================================================
-- authenticated: dono humano via dashboard. As políticas RLS (init_schema §10) já
-- restringem às linhas do próprio `owner_id`; aqui concedemos os privilégios de tabela
-- correspondentes. O dono pode escrever rele_aberto/p_max/config (diferente do device).
-- device_credentials NÃO é concedida a authenticated (só service_role).
-- ============================================================
grant select, insert, update, delete on table residencias        to authenticated;
grant select, insert, update, delete on table tomadas            to authenticated;
grant select                          on table medicoes           to authenticated;
grant select                          on table eventos            to authenticated;
grant select, insert, update, delete on table provisioning_claims to authenticated;
