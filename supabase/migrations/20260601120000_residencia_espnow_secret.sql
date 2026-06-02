-- ============================================================
-- residencia_secrets: chave HMAC compartilhada por residência para autenticar os
-- heartbeats ESP-NOW entre as tomadas (protocolo v3).
--
-- Princípio VIII: este segredo NÃO trafega pela Data API. RLS default-deny e SEM grant
-- ao role `device` -> o firmware nunca o lê via PostgREST. A chave é entregue ao
-- dispositivo SOMENTE na resposta da Edge Function `/provisionar` (service_role), uma vez,
-- e guardada em LittleFS. Rotacionar = apagar a linha (a Edge Function gera outra) e
-- re-provisionar os devices da residência.
-- ============================================================
create table if not exists residencia_secrets (
  residencia_id  uuid primary key references residencias(id) on delete cascade,
  espnow_key_hex text not null,                 -- 32 bytes em hex (64 chars) — chave HMAC da casa
  criado_em      timestamptz not null default now()
);

alter table residencia_secrets enable row level security;

-- Default-deny explícito: nenhuma policy é criada e nenhum privilégio é concedido ao
-- role `device`. Só o service_role (que ignora RLS) acessa. O revoke é defensivo caso um
-- grant amplo exista no schema.
revoke all on residencia_secrets from device;
