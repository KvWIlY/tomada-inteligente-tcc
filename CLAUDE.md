<!-- SPECKIT START -->
Active feature plan: specs/003-fallback-coordenacao-nuvem/plan.md
Related (003): spec.md, research.md, data-model.md, contracts/ (cloud-fallback.md),
quickstart.md in the same dir. Wires the orphan `buscar_todas_tomadas` into the decision
cycle: when online AND ESP-NOW has zero live peers, sum sibling `p_medida_atual` (fresh
via `ultimo_uso_ativo_ts`, excluding self) as `carga_fixa` subtracted from P_max — a
degraded, conservative coordination path for mesh partition. Shallow (not a MILP decision
var); gated on `peers_vivos.is_empty()` to avoid MAC↔UUID dedup. Pure logic host-tested in
`tomada-solver::coordenacao`. No schema/auth/flash changes.
Ancestor feature (002): specs/002-refinamento-supabase/plan.md — per-device JWT (Princ.
VIII), batch+confirm+idempotent sync, config/rele pull, end-to-end RLS verification.
Ancestor feature (001): specs/001-otimizacao-tomadas-milp/plan.md — spec.md, research.md,
data-model.md, contracts/ (supabase-schema.sql, provisioning-api.md, espnow-protocol.md,
solver-api.md), quickstart.md. The 001 schema + provisioning contracts remain authoritative.

Project: ESP32 (Rust/esp-idf) smart-outlet energy optimizer. MILP model (review_modelo
.pdf topics 1-4) in a host-testable `tomada-solver` crate (minilp + branch-and-bound).
Sensors ZMPT101B (voltage) + SCT-013 20A/1V (current). Relay actuation Option A
(OFF=open, STD/ON=closed). ESP-NOW inter-node coordination for the global P_max cap.
Supabase as cloud source of truth (adapt firmware/src/comunicacao/supabase.rs.template).
Security (Principle VIII): device identity = MAC -> tomada_id UUID; auth = per-device
JWT (role `device`, claims residencia_id/tomada_id) with RLS default-deny; JWT signed
server-side by a provisioning Edge Function (service_role); NO shared secret in the .bin.
Constitution: .specify/memory/constitution.md (v1.2.0). Read the plan before coding.
<!-- SPECKIT END -->
