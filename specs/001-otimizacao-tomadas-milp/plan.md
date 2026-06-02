# Implementation Plan: Otimização Energética de Tomadas Inteligentes (Modelo MILP Multi-Nó)

**Branch**: `001-otimizacao-tomadas-milp` | **Date**: 2026-05-29 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/001-otimizacao-tomadas-milp/spec.md`

## Summary

Sistema de tomadas inteligentes (ESP32 + Rust) que decide o estado de cada tomada
(`OFF`/`STD`/`ON`) resolvendo o modelo MILP dos tópicos 1–4 do `review_modelo.pdf` sem
alterações: minimiza consumo global sob o teto elétrico `P_max` (knapsack, R2),
preservando conforto via probabilidade histórica `Π_{i,t}` e absorvendo conflitos com
variáveis de folga (R3/R5, sempre factível). Cada tomada mede tensão (ZMPT101B) e
corrente (SCT-013 20A/1V), atua um relé binário (Opção A: `OFF`=aberto, `STD`/`ON`=
fechado), coordena o teto global com as demais via ESP-NOW, persiste medições de 1 min
em lote no Supabase e mantém um buffer local de 7 dias com purga a cada 5 dias.
Cada nó se identifica pelo MAC (→ `tomada_id` UUID) e autentica no Supabase com um
JWT por-dispositivo (RLS default-deny por claim), provisionado em runtime via portal
AP + Edge Function — sem credencial compartilhada no binário (Princípio VIII).

Abordagem técnica: extrair o modelo e o solver para um crate `tomada-solver`
testável no host (cargo test puro), reaproveitar o cliente `supabase.rs.template` já
alinhado ao domínio, e adicionar módulos de sensores, atuação, ESP-NOW e retenção ao
firmware existente.

## Technical Context

**Language/Version**: Rust edition 2021, `rust-version = 1.82`; toolchain `esp` (Xtensa)
para o firmware; toolchain stable do host para o crate do solver.

**Primary Dependencies**: `esp-idf-svc 0.52.1` (std, binstart), `embedded-svc 0.29`,
`serde`/`serde_json` (JSON manual onde houver `f32` — bug LLVM Xtensa), `chrono`,
`anyhow`; **solver**: `minilp` (LP contínuo) + camada própria de branch-and-bound para
integralidade binária; ESP-NOW via `esp-idf-svc`/`esp-idf-sys`.

**Storage**: LittleFS no ESP32 (buffer local em lote + credenciais provisionadas:
`tomada_id`, `residencia_id`, `device_secret`, cache do JWT); Supabase (PostgREST) como
fonte única de verdade na nuvem.

**Identidade/Auth**: identidade por MAC (`esp_efuse_mac_get_default`) → `tomada_id` UUID;
autenticação por **JWT por-dispositivo** (role `device`, claims `residencia_id`/
`tomada_id`) com **RLS default-deny**; emissão/renovação numa **Edge Function**
(`service_role`, assina o JWT server-side). Dashboard autentica como usuário humano
(`authenticated`, isolado por `owner_id`). Ver `contracts/provisioning-api.md`.

**Testing**: `cargo test` no host para o crate `tomada-solver` (lógica pura, validação
do modelo contra R1–R5 e cenários sintéticos); smoke tests on-device para sensores,
atuação e sincronização.

**Target Platform**: ESP32 (Xtensa, esp-idf, std); host Linux/Windows para testes do
solver e do modelo.

**Project Type**: multi-componente — firmware embarcado (Rust/ESP32) + crate de solver
compartilhado + banco em nuvem (Supabase) + dashboard web (Vite + TypeScript).

**Performance Goals**: decisão de estado atualizada em ≤10 s após nova leitura
(SC-008); solver trivial em tempo para `N ≤ 8` e 3 estados (24 variáveis binárias).

**Constraints**: proteção de flash (sem escrita por minuto — buffer em RAM + flush em
lote); `serde_json` em `opt-level = 0` e perfil dev `opt-level = 0` (bug Xtensa LLVM em
`[2 x float]`); stack da main task 40 KB; binário ~2,4 MB na partição factory de
3,125 MB; offline-first (opera sem nuvem, reconcilia ao reconectar).

**Scale/Scope**: até ~8 tomadas (`N ≤ 8`) por residência, dentro do limite de peers do
ESP-NOW; faixas horárias de `Π` (24/dia); retenção local de 7 dias com purga a cada 5.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

Avaliação contra a Constituição v1.2.0:

| Princípio | Conformidade do plano |
|-----------|------------------------|
| I. Modelagem rigorosa e sempre factível | ✅ Solver MILP com `x_{i,s} ∈ {0,1}` (branch-and-bound); R1/R2 hard, R3/R5 soft com folgas → sempre factível. Decisão fora de `IF/ELSE`. |
| II. Complexidade justificada | ✅ Solver justificado pelo acoplamento `P_max` (R2). Reuso do `supabase.rs.template` e crate único; sem ML adicional (fora de escopo). A Edge Function de provisionamento remove uma falha de *safety*, não adiciona complexidade gratuita. |
| III. Confiabilidade do hardware | ✅ Amostra em RAM; persistência de 1 min bufferizada e gravada em lote; purga 5d / cap 7d. |
| IV. Inteligência preditiva por histórico | ✅ `Π_{i,t}` horário sobre janela `D_i` dias; peso `W_i`/criticidade (`is_critical`). |
| V. Coordenação multi-nó resiliente | ✅ ESP-NOW para agregar consumo e honrar `P_max`; degradação local segura (research §2). |
| VI. Supabase como fonte única de verdade | ✅ Schema modelado (contracts/); offline-first; identidade do device por MAC→UUID estabelecida no provisionamento. |
| VII. Observabilidade e reprodutibilidade | ✅ Decisões e penalidades registradas em `eventos`; solver determinístico, reproduzível a partir das entradas. |
| VIII. Segurança e isolamento multi-inquilino | ✅ Identidade (MAC) ≠ autenticação (JWT por-device); RLS default-deny por claim; menor privilégio por coluna (`device` não escreve `rele_aberto`/`p_max`); segredo de assinatura só no servidor (Edge Function); `.bin` genérico; escritas safety-critical protegidas. Ver `contracts/provisioning-api.md` e `supabase-schema.sql`. |

**Resultado**: PASS. Único risco listado na constituição (minilp LP vs. integralidade
binária) é resolvido no Phase 0 (branch-and-bound). O modelo de auth foi corrigido para
satisfazer o Princípio VIII (era anon key compartilhada). Sem violações em *Complexity
Tracking*.

## Project Structure

### Documentation (this feature)

```text
specs/001-otimizacao-tomadas-milp/
├── plan.md              # Este arquivo
├── spec.md              # Especificação + Clarifications
├── research.md          # Phase 0 (decisões técnicas)
├── data-model.md        # Phase 1 (entidades)
├── quickstart.md        # Phase 1 (build/test/flash/deploy)
├── contracts/
│   ├── supabase-schema.sql   # Tabelas + RLS default-deny + role device (Princ. VIII)
│   ├── provisioning-api.md   # Edge Function provisionar/renovar (MAC→JWT por-device)
│   ├── espnow-protocol.md    # Formato das mensagens entre ESP32
│   └── solver-api.md         # SolverInput/SolverOutput do crate tomada-solver
└── checklists/
    └── requirements.md       # Checklist de qualidade da spec
```

### Source Code (repository root)

```text
Cargo.toml                      # workspace: [firmware, tomada-solver]

tomada-solver/                  # NOVO crate — modelo + solver, testável no host
├── Cargo.toml                  # dep: minilp (host + esp-idf std)
├── src/
│   ├── lib.rs
│   ├── tipos.rs                # Estado {Off,Std,On}, SolverInput, SolverOutput
│   ├── modelo.rs               # parâmetros C_{i,s}, P_max, U_i, F_i, Π, W_i, A_{i,t}
│   └── solver.rs               # função objetivo + R1–R5 + branch-and-bound (minilp)
└── tests/
    └── modelo.rs               # cenários sintéticos: factibilidade, P_max, R3/R4/R5

firmware/
├── Cargo.toml                  # + dep: tomada-solver (path), esp-now
└── src/
    ├── main.rs                 # orquestra ciclo: medir → decidir → atuar → sincronizar
    ├── config_nuvem.rs         # só SUPABASE_URL + Wi-Fi bootstrap (sem credencial por-device)
    ├── sensores/               # NOVO
    │   ├── mod.rs
    │   ├── zmpt101b.rs         # tensão RMS (ADC)
    │   ├── sct013.rs           # corrente RMS (ADC, 20A/1V)
    │   └── potencia.rs         # P = Vrms·Irms; deriva U_i, F_i, calibração cstd/con
    ├── decisao/                # NOVO
    │   ├── mod.rs
    │   ├── atuador.rs          # relé (Opção A: Off=low, Std/On=high)
    │   └── ciclo.rs            # monta SolverInput, chama solver, aplica saída
    ├── comunicacao/
    │   ├── mod.rs
    │   ├── supabase.rs         # adaptado: Bearer <JWT por-device> + renovação (não anon key)
    │   ├── provisionamento.rs  # NOVO — chama Edge Function provisionar/renovar
    │   ├── espnow.rs           # NOVO — coordenação multi-nó (P_max)
    │   ├── wifi.rs / wifi_provisioning.rs / sntp.rs   # existentes (AP coleta claim-code)
    └── persistencia/
        ├── mod.rs / littlefs.rs            # existentes
        ├── medicao.rs         # NOVO — Medicao + buffer RAM + flush em lote
        ├── config.rs          # NOVO — Tomada (schedule, pi_historico, w_i, p_max…)
        ├── credenciais.rs     # NOVO — tomada_id/residencia_id/device_secret + cache JWT
        └── retencao.rs        # NOVO — purga 5d / cap 7d, só dados confirmados

supabase/functions/provisionar/   # NOVO — Edge Function (Deno): provisionar + renovar
└── index.ts                       # assina JWT por-device (service_role); ver provisioning-api.md

dashboard/                     # adaptar views ao domínio (Supabase JS)
└── src/ ...
```

**Structure Decision**: Workspace Rust com um **crate de solver separado**
(`tomada-solver`) que compila tanto no host (para `cargo test` do modelo, sem hardware)
quanto sob esp-idf std (linkado no firmware). Isso satisfaz o Princípio VII
(reprodutibilidade/testes) e o Princípio I (modelo isolado, auditável). O firmware
mantém a infra portada (LittleFS/Wi-Fi/SNTP) e ganha módulos de sensores, decisão,
ESP-NOW e retenção. O cliente Supabase é adaptado do `template` existente.

## Complexity Tracking

> Sem violações da Constituição que exijam justificativa. A introdução do crate
> `tomada-solver` reduz complexidade (isola e testa o modelo) em vez de aumentá-la.

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| (nenhuma) | — | — |
