-- Supabase schema — Otimização de Tomadas Inteligentes (TCC)
-- Feature: 001-otimizacao-tomadas-milp
-- Fonte única de verdade na nuvem (Constituição, Princípio VI).
-- Segurança e isolamento multi-inquilino (Constituição, Princípio VIII):
--   * Identidade do dispositivo = MAC (coluna `mac` única) → mapeado a um UUID.
--   * Autenticação = JWT por-dispositivo (role `device`), emitido server-side pela
--     Edge Function de provisionamento (ver contracts/provisioning-api.md). O segredo
--     de assinatura do JWT NUNCA vive no firmware.
--   * RLS DEFAULT-DENY em todas as tabelas; acesso concedido só por políticas que
--     chaveiam em claims do JWT (`residencia_id`, `tomada_id`).
--   * Menor privilégio por COLUNA: o role `device` não pode escrever `rele_aberto`,
--     `p_max`, `mac`, `residencia_id` — só o dono humano (dashboard) pode.
--
-- Aplicar no SQL Editor do Supabase (idempotente; pode rodar mais de uma vez).

-- ============================================================
-- 0. Role de dispositivo (least privilege)
--    PostgREST faz SET ROLE a partir do claim `role` do JWT. Um JWT de device
--    carrega `"role":"device"`; um usuário do dashboard usa `authenticated`.
-- ============================================================
do $$
begin
  if not exists (select from pg_roles where rolname = 'device') then
    create role device nologin;
  end if;
end$$;
-- Permite ao PostgREST assumir o role `device` a partir do JWT.
grant device to authenticator;

-- ============================================================
-- 1. residencias: agrupa o conjunto I de tomadas e define o teto P_max.
--    `owner_id` amarra a residência a um usuário humano (dashboard / Supabase Auth).
-- ============================================================
create table if not exists residencias (
  id           uuid primary key default gen_random_uuid(),
  owner_id     uuid not null references auth.users(id) on delete cascade,
  nome         text not null,
  p_max        integer not null check (p_max > 0),  -- W, teto global (R2)
  criado_em    timestamptz not null default now()
);
create index if not exists idx_residencias_owner on residencias(owner_id);

-- ============================================================
-- 2. tomadas: config (parâmetros do modelo) + telemetria compartilhada.
--    `mac`  = identidade de hardware (Princípio VIII); única e imutável por chip.
--    `config` espelha o struct Tomada do firmware (JSON) para sync por versão.
-- ============================================================
create table if not exists tomadas (
  id                    uuid primary key default gen_random_uuid(),
  residencia_id         uuid not null references residencias(id) on delete cascade,
  mac                   text not null unique,           -- MAC efuse (identidade, NÃO segredo)
  nome                  text not null,

  -- decisão / atuação
  estado                text not null default 'OFF' check (estado in ('OFF','STD','ON')),
  rele_aberto           boolean not null default true,  -- comando manual do dashboard

  -- calibração de consumo (C_{i,s})
  cstd                  real not null default 0,  -- C_{i,STD} (W)
  con                   real not null default 0,  -- C_{i,ON}  (W); C_{i,OFF}=0

  -- telemetria (compartilhada também via ESP-NOW; aqui p/ dashboard/fallback)
  p_medida_atual        real not null default 0,
  ultimo_uso_ativo_ts   bigint not null default 0,

  -- parâmetros do modelo, versionados p/ pull pelo firmware
  config                jsonb not null default '{}'::jsonb,
  config_version        bigint not null default 0,

  criado_em             timestamptz not null default now()
);
-- Estrutura esperada de `config` (jsonb):
--   {
--     "is_critical": bool,            -- W_i = M
--     "w_i": int,                     -- peso de criticidade
--     "p_max": int,                   -- redundante p/ o nó (= residencias.p_max)
--     "t_limite": int,                -- T_limite,i (s) -> F_i
--     "schedule": [bool; 24],         -- A_{i,t} por hora
--     "pi_historico": [float; 24],    -- Π_{i,t} por hora (0..1)
--     "p_limiar_detectado_auto": float -- P_limiar,i -> U_i
--   }
create index if not exists idx_tomadas_residencia on tomadas(residencia_id);

-- ============================================================
-- 3. medicoes: série temporal (1/min, enviada em lote)
-- ============================================================
create table if not exists medicoes (
  id           bigint generated always as identity primary key,
  tomada_id    uuid not null references tomadas(id) on delete cascade,
  timestamp    bigint not null,                 -- epoch (SNTP)
  p_medida     real not null check (p_medida >= 0),
  estado       text not null check (estado in ('OFF','STD','ON')),
  rele_aberto  boolean not null,
  criado_em    timestamptz not null default now(),
  unique (tomada_id, timestamp)
);
create index if not exists idx_medicoes_tomada_ts on medicoes(tomada_id, timestamp desc);

-- ============================================================
-- 4. eventos: observabilidade — decisões e penalidades (R3/R4/R5) — Princípio VII
-- ============================================================
create table if not exists eventos (
  id           bigint generated always as identity primary key,
  tomada_id    uuid references tomadas(id) on delete cascade,
  timestamp    bigint not null,
  tipo         text not null,   -- chaveamento | penalidade_uso | penalidade_age |
                                --   fantasma_eliminado | solver
  severidade   text not null default 'info' check (severidade in ('info','warn','erro')),
  titulo       text not null,
  detalhes     text,            -- entradas + Z + estados + folgas (reprodutível)
  criado_em    timestamptz not null default now()
);
create index if not exists idx_eventos_tomada_ts on eventos(tomada_id, timestamp desc);

-- ============================================================
-- 5. device_credentials: segredo durável (refresh) por-dispositivo.
--    Guarda APENAS o hash do segredo (nunca o texto). A Edge Function compara o
--    segredo apresentado contra `secret_hash`. Revogar = setar `revoked_at`.
-- ============================================================
create table if not exists device_credentials (
  tomada_id    uuid primary key references tomadas(id) on delete cascade,
  secret_hash  text not null,                 -- ex.: digest sha256(segredo) hex
  criado_em    timestamptz not null default now(),
  revoked_at   timestamptz,                   -- not null => credencial inválida
  ultima_renovacao timestamptz
);
-- Nunca exposta a `device`/`authenticated`: só o service_role (Edge Function) acessa.

-- ============================================================
-- 6. provisioning_claims: claim-codes de pareamento por residência.
--    O usuário gera um código no dashboard; o device o apresenta no portal AP.
--    Guarda só o hash; uso único; expira.
-- ============================================================
create table if not exists provisioning_claims (
  id            uuid primary key default gen_random_uuid(),
  residencia_id uuid not null references residencias(id) on delete cascade,
  code_hash     text not null,
  expira_em     timestamptz not null,
  usado_em      timestamptz,
  criado_em     timestamptz not null default now()
);
create index if not exists idx_claims_residencia on provisioning_claims(residencia_id);

-- ============================================================
-- 7. RLS — DEFAULT DENY. Habilita RLS; sem política => sem acesso.
-- ============================================================
alter table residencias         enable row level security;
alter table tomadas             enable row level security;
alter table medicoes            enable row level security;
alter table eventos             enable row level security;
alter table device_credentials  enable row level security;
alter table provisioning_claims enable row level security;
-- Força RLS inclusive para donos de tabela (defesa em profundidade).
alter table device_credentials  force row level security;
alter table provisioning_claims force row level security;

-- ----- Claims auxiliares do JWT (helpers) -----
-- auth.jwt() ->> 'residencia_id' e ->> 'tomada_id' são preenchidos pela Edge Function.

-- ============================================================
-- 8. Privilégios de COLUNA para o role `device` (menor privilégio).
--    O device NÃO recebe update de rele_aberto / p_max / mac / residencia_id.
-- ============================================================
grant select on residencias to device;          -- lê o próprio P_max
grant select on tomadas     to device;          -- lê irmãs (fallback multi-nó via nuvem)
grant insert on medicoes    to device;
grant insert on eventos     to device;
grant update (estado, cstd, con, p_medida_atual, ultimo_uso_ativo_ts,
              config, config_version) on tomadas to device;
-- (sem grant em device_credentials / provisioning_claims => device nunca os toca)

-- ============================================================
-- 9. Políticas RLS — role `device` (isolado por claims do JWT)
-- ============================================================

-- residencias: o device lê só a sua.
drop policy if exists dev_residencias_sel on residencias;
create policy dev_residencias_sel on residencias
  for select to device
  using (id = (auth.jwt() ->> 'residencia_id')::uuid);

-- tomadas: lê todas as da sua residência (necessário p/ fallback multi-nó pela nuvem)…
drop policy if exists dev_tomadas_sel on tomadas;
create policy dev_tomadas_sel on tomadas
  for select to device
  using (residencia_id = (auth.jwt() ->> 'residencia_id')::uuid);

-- …mas só atualiza a PRÓPRIA linha (id == claim tomada_id). Combinado com o grant de
-- coluna acima, garante que um nó nunca escreve estado/telemetria de outro.
drop policy if exists dev_tomadas_upd on tomadas;
create policy dev_tomadas_upd on tomadas
  for update to device
  using      (id = (auth.jwt() ->> 'tomada_id')::uuid)
  with check (id = (auth.jwt() ->> 'tomada_id')::uuid);

-- medicoes: insere só medições da própria tomada; lê as da sua residência.
drop policy if exists dev_medicoes_ins on medicoes;
create policy dev_medicoes_ins on medicoes
  for insert to device
  with check (tomada_id = (auth.jwt() ->> 'tomada_id')::uuid);

drop policy if exists dev_medicoes_sel on medicoes;
create policy dev_medicoes_sel on medicoes
  for select to device
  using (exists (
    select 1 from tomadas t
    where t.id = medicoes.tomada_id
      and t.residencia_id = (auth.jwt() ->> 'residencia_id')::uuid));

-- eventos: insere só eventos da própria tomada; lê os da sua residência.
drop policy if exists dev_eventos_ins on eventos;
create policy dev_eventos_ins on eventos
  for insert to device
  with check (tomada_id = (auth.jwt() ->> 'tomada_id')::uuid);

drop policy if exists dev_eventos_sel on eventos;
create policy dev_eventos_sel on eventos
  for select to device
  using (exists (
    select 1 from tomadas t
    where t.id = eventos.tomada_id
      and t.residencia_id = (auth.jwt() ->> 'residencia_id')::uuid));

-- ============================================================
-- 10. Políticas RLS — role `authenticated` (dashboard / dono humano)
--     O dono enxerga/gerencia apenas as residências que possui (owner_id = auth.uid()).
--     Só o dono escreve rele_aberto, p_max e config.
-- ============================================================
drop policy if exists usr_residencias_all on residencias;
create policy usr_residencias_all on residencias
  for all to authenticated
  using      (owner_id = auth.uid())
  with check (owner_id = auth.uid());

drop policy if exists usr_tomadas_all on tomadas;
create policy usr_tomadas_all on tomadas
  for all to authenticated
  using      (exists (select 1 from residencias r
                       where r.id = tomadas.residencia_id and r.owner_id = auth.uid()))
  with check (exists (select 1 from residencias r
                       where r.id = tomadas.residencia_id and r.owner_id = auth.uid()));

drop policy if exists usr_medicoes_sel on medicoes;
create policy usr_medicoes_sel on medicoes
  for select to authenticated
  using (exists (select 1 from tomadas t join residencias r on r.id = t.residencia_id
                 where t.id = medicoes.tomada_id and r.owner_id = auth.uid()));

drop policy if exists usr_eventos_sel on eventos;
create policy usr_eventos_sel on eventos
  for select to authenticated
  using (exists (select 1 from tomadas t join residencias r on r.id = t.residencia_id
                 where t.id = eventos.tomada_id and r.owner_id = auth.uid()));

-- provisioning_claims: o dono cria/consulta claim-codes da própria residência
-- (o device NUNCA acessa esta tabela; a Edge Function valida via service_role).
drop policy if exists usr_claims_all on provisioning_claims;
create policy usr_claims_all on provisioning_claims
  for all to authenticated
  using      (exists (select 1 from residencias r
                      where r.id = provisioning_claims.residencia_id and r.owner_id = auth.uid()))
  with check (exists (select 1 from residencias r
                      where r.id = provisioning_claims.residencia_id and r.owner_id = auth.uid()));

-- device_credentials: nenhuma política para device/authenticated => acesso só por
-- service_role (a Edge Function de provisionamento), que ignora RLS por definição.

-- ============================================================
-- 11. Trava extra: impedir que `anon` faça qualquer coisa.
--     (Sem grants e sem políticas, `anon` já é negado; explicitar por clareza.)
-- ============================================================
revoke all on residencias, tomadas, medicoes, eventos,
              device_credentials, provisioning_claims from anon;
