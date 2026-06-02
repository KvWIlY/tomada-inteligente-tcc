# Quickstart — Otimização de Tomadas Inteligentes (TCC)

Feature: 001-otimizacao-tomadas-milp

Como construir, testar e executar cada componente. Pré-requisitos: toolchain `esp`
(Rust/Xtensa, via `rust-toolchain.toml`), Node.js, e um projeto Supabase.

## 1. Solver (host — sem hardware)

O crate `tomada-solver` roda e testa na sua máquina, validando o modelo (R1–R5).

```powershell
cd C:\y
cargo test -p tomada-solver        # roda os casos de solver-api.md
```

Espera-se: todos os testes verdes; nenhum cenário infactível; B&B == enumeração para
N pequeno.

## 2. Banco (Supabase)

1. Crie um projeto Supabase.
2. Aplique o schema (cria tabelas, role `device`, RLS default-deny — Princípio VIII):
   ```
   specs/001-otimizacao-tomadas-milp/contracts/supabase-schema.sql
   ```
   (SQL Editor → cole → run. Idempotente.)
3. Crie o usuário-dono (Supabase Auth) e uma `residencia` com `owner_id = auth.uid()`
   e `p_max`. (Pelo dashboard autenticado; as policies isolam por dono.)
4. **NÃO** crie tomadas à mão: cada ESP32 se auto-registra pelo MAC no provisionamento.
5. Deploy da Edge Function de provisionamento (assina o JWT por-device server-side):
   ```powershell
   supabase functions deploy provisionar
   supabase secrets set SUPABASE_JWT_SECRET=... SERVICE_ROLE_KEY=...
   ```
   Contrato: `contracts/provisioning-api.md`.
6. Anote o `Project URL` (público). **Não** distribua `service_role` nem o JWT secret —
   eles vivem só no servidor.

## 3. Firmware (ESP32)

O `.bin` é **genérico** (sem credencial por-dispositivo). Só Wi-Fi de bootstrap e a URL:

```powershell
cd C:\y\firmware
copy .env.example .env   # preencha APENAS:
# WIFI_SSID, WIFI_PASSWORD   (bootstrap; o portal AP também coleta em runtime)
# SUPABASE_URL
cargo build
cargo run                # flash + monitor
```

**Provisionamento (1ª vez, por chip):**
1. No dashboard, gere um **claim-code** para a residência (uso único, expira).
2. Ligue o ESP32; conecte ao AP de provisionamento; informe Wi-Fi + claim-code.
3. O device chama `provisionar(mac, claim_code)`, recebe `{tomada_id, residencia_id,
   device_secret, jwt}` e grava em LittleFS. A partir daí fala com o PostgREST via
   `Authorization: Bearer <jwt>` e renova com `renovar(tomada_id, device_secret)`.
   Nenhum segredo por-dispositivo fica no binário.

Ligações de hardware (por tomada):
- **ZMPT101B** (tensão) → canal ADC A; **SCT-013 20A/1V** (corrente) → canal ADC B.
- **Relé** no GPIO de atuação: `HIGH`=fechado (`ON`/`STD`), `LOW`=aberto (`OFF`).

Ciclo esperado no log: `medir → decidir (solver) → atuar relé → broadcast ESP-NOW →
sincronizar Supabase (lote)`.

## 4. Multi-nó (ESP-NOW)

- Provisione ≥2 ESP32 na **mesma residência** (mesmo claim-code de residência ou claims
  distintos da mesma `residencia_id`); cada um recebe seu `tomada_id` pelo MAC.
- No log de cada nó deve aparecer a recepção de heartbeats dos peers e a decisão global
  respeitando `P_max` (R2). Desligue um nó e verifique a degradação segura.

## 5. Dashboard

```powershell
cd C:\y\dashboard
npm install
npm run dev
```

Configurar `SUPABASE_URL`/`anon key` (a anon key **no browser é normal e segura**: o
usuário faz login via Supabase Auth e a RLS isola por `owner_id = auth.uid()`). Telas:
login do dono, geração de claim-code para provisionar tomadas, estado/consumo por
tomada, histórico (`medicoes`), eventos/penalidades (`eventos`), toggle manual
(`rele_aberto`) e config (schedule, criticidade).

## 6. Validação ponta-a-ponta (mapeada aos Success Criteria)

| Verificação | SC |
|-------------|----|
| Nenhuma decisão infactível nos testes do solver | SC-001 |
| `Σ C_{i,estado} ≤ P_max` em toda decisão | SC-002 |
| Um estado por tomada | SC-003 |
| Penalidades (R3/R5) aparecem em `eventos` | SC-004 |
| Dados locais nunca > 7 dias | SC-005 |
| Decisão segura com um nó ausente | SC-006 |
| Decisão reproduzível a partir das entradas logadas | SC-007 |
| Decisão atualiza ≤10 s após leitura | SC-008 |
| Purga não apaga dado não confirmado (dentro de 7d) | SC-009 |
