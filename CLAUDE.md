<!-- SPECKIT START -->
Active feature plan: specs/002-refinamento-supabase/plan.md
Related (002): spec.md, research.md, data-model.md, contracts/ (supabase-client.md,
rls-verification.md), quickstart.md in the same dir. Refines the Supabase layer of 001:
migrates firmware client from shared anon key to per-device JWT (Princ. VIII), adds
batch+confirm+idempotent sync, config/rele pull, and end-to-end RLS verification.
Parent feature (001): specs/001-otimizacao-tomadas-milp/plan.md — spec.md, research.md,
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
