# y/ — Nova versão (reaproveita infra do TCC anterior)

Projeto novo, ainda sem domínio definido. A infra abaixo já foi portada e validada no projeto anterior (`../firmware`, `../dashboard`):

- **Firmware (Rust + ESP32)** — LittleFS, Wi-Fi (STA + AP de provisionamento), SNTP.
- **Dashboard (Vite + TypeScript + Supabase JS)** — copiado integralmente.
- **Cliente Supabase HTTPS (firmware)** — preservado como referência em `firmware/src/comunicacao/supabase.rs.template` (acoplado ao domínio antigo; renomear e adaptar quando o novo domínio estiver definido).

## Estrutura

```
y/
├── Cargo.toml                 # workspace (membro: firmware)
├── firmware/
│   ├── Cargo.toml             # deps mínimas (esp-idf-svc, anyhow, serde, chrono, dotenvy)
│   ├── partitions.csv         # nvs/phy/factory/storage(littlefs)
│   ├── sdkconfig.defaults     # stack 40KB, FreeRTOS 1ms, particao LittleFS
│   ├── rust-toolchain.toml    # toolchain esp
│   ├── build.rs
│   ├── .env.example           # copie p/ .env e preencha
│   └── src/
│       ├── main.rs            # smoke test: mount LittleFS + Wi-Fi + SNTP + heartbeat
│       ├── config_nuvem.rs    # constantes vindas do .env em compile-time
│       ├── persistencia/
│       │   ├── mod.rs
│       │   └── littlefs.rs    # mount/info/base
│       └── comunicacao/
│           ├── mod.rs
│           ├── wifi.rs                  # STA + reconexao
│           ├── wifi_provisioning.rs     # AP fallback p/ provisionar SSID
│           ├── sntp.rs                  # sincronizacao de relogio
│           └── supabase.rs.template     # REFERENCIA: cliente HTTPS Supabase (acoplado ao dominio antigo)
└── dashboard/                 # Vite + TS + Supabase JS (cópia do projeto anterior)
```

## Setup

### Firmware

```powershell
cd C:\y\firmware
copy .env.example .env       # preencha WIFI_SSID, WIFI_PASSWORD, PROJECT_URL, PUBLIC_KEY
cargo build                  # toolchain esp já configurado em rust-toolchain.toml
cargo run                    # flash + monitor (configurado pelo esp-idf-svc)
```

### Dashboard

```powershell
cd C:\y\dashboard
npm install
npm run dev
```

## Notas sobre a configuração LittleFS

Já vem aplicado o que evita os problemas conhecidos do projeto anterior:

- `sdkconfig.defaults` aponta `CONFIG_PARTITION_TABLE_CUSTOM_FILENAME` para o `partitions.csv` deste crate (caminho absoluto Windows).
- `Cargo.toml` raiz força `serde_json` em `opt-level = 0` (bug LLVM Xtensa em `[2 x float]` constant pools — só dispara em release com opt-level ≥ 1).
- `[profile.dev] opt-level = 0` pelo mesmo motivo. Binário ~2.4 MB cabe na partição factory de 3.125 MB.
- Stack da main task em 40 KB — necessário para Wi-Fi/lwIP + alocações Rust.
- LittleFS via componente `joltwallet/littlefs` v1.21.1 (declarado em `[package.metadata.esp-idf-sys]`).

## Próximos passos

1. Defina o domínio do novo projeto.
2. Renomeie `supabase.rs.template` → `supabase.rs` e adapte structs/endpoints (remova `Medicao`/`Tomada`).
3. Adicione `pub mod supabase;` em `comunicacao/mod.rs`.
4. Crie os módulos de domínio em `persistencia/` (use `littlefs::base()` = `/lfs` como ponto de montagem).
5. Refatore o `dashboard/src/` para as novas views.
