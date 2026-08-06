# Tomada inteligente — otimizador de consumo fantasma (TCC)

Rede de tomadas ESP32 que corta o consumo em *standby* sem estourar o teto elétrico da casa.
Cada nó mede tensão/corrente, decide o próprio estado resolvendo um **MILP** local, coordena-se
com as irmãs por **ESP-NOW** e sincroniza com um **backend próprio** na LAN.

## Componentes

| Diretório | O quê | Alvo |
|-----------|-------|------|
| `firmware/` | Rust + esp-idf no ESP32: sensores, solver, relé, ESP-NOW, cliente do backend | Xtensa |
| `tomada-solver/` | Lógica pura host-testável: modelo MILP (minilp + branch-and-bound), coordenação, sync | host |
| `backend/` | Rust + Actix-Web sobre MySQL: **fonte única de verdade**, auth própria (JWT), autorização default-deny | host |
| `frontend/` | Vite + TypeScript: dashboard do dono | browser |
| `specs/` | Especificações speckit (001–004) — leia o plano da feature ativa antes de codar | — |

`backend/` é um **projeto cargo independente** — não é membro do workspace da raiz, que é do
embarcado (toolchain esp + workarounds do bug LLVM Xtensa). Ver comentário no `Cargo.toml` raiz.

## Arquitetura de dados e segurança

- **Fonte de verdade**: MySQL, acessível **apenas** pelo backend. Nenhum cliente fala com o banco.
- **Identidade do device**: MAC → `tomada_id`. O MAC identifica, não autentica.
- **Autenticação**: JWT HS256 emitido pelo backend, para usuários (dashboard) e devices (firmware).
- **Autorização**: default-deny em todo handler, pelo claim do token. Não há RLS.
- **Segredos**: `JWT_SECRET` e a credencial do MySQL vivem **só** em `backend/.env`. O `.bin` do
  firmware é genérico — sem chave de API, sem id por dispositivo, sem claim-code.
- **Pareamento com nuvem**: o dono gera um claim-code de uso único no dashboard e digita no
  Portal AP da tomada; o backend troca por `device_secret` durável + JWT.
- **Revogação efetiva**: revogar um dispositivo no dashboard corta o acesso dele à API *e*
  rotaciona a chave de coordenação da casa na mesma transação — as tomadas legítimas recebem a
  nova no `GET /config` seguinte, sem reprovisionar.

## Operação sem internet (feature 005)

A tomada **não depende de internet, servidor ou hora certa** para funcionar:

- **Modo sem nuvem**: no Portal AP, escolha "Sem nuvem". A tomada cunha a própria identidade e a
  chave da casa, e entra no ciclo medir → decidir → atuar com os limites padrão. Dá para conectar
  à nuvem depois, pelo mesmo portal, sem regravar o firmware.
- **Pareamento local entre tomadas**: aperte o botão BOOT da tomada que já funciona (abre uma
  janela de 60 s) e digite o **PIN de 8 caracteres** dela no portal da tomada nova. As duas fazem
  um acordo de chaves autenticado pelo PIN (SPAKE2) e a chave da casa viaja cifrada pelo rádio —
  o PIN nunca trafega. O PIN de cada tomada aparece no console serial no boot e no seu portal.
- **Relógio**: sem NTP a tomada opera pelo tempo desde o boot; as regras que dependem da hora do
  dia (agenda e histórico) ficam suspensas até a hora certa chegar, e aí os registros pendentes
  têm o carimbo corrigido pelo deslocamento.
- **Queda de energia**: se o roteador voltar depois da tomada, o portal continua tentando a rede
  salva a cada 30 s e fecha sozinho quando ela volta — sem intervenção.

## Setup

### Backend
```powershell
cd C:\y\backend
mysql -u root -p < scripts\bootstrap-mysql.sql   # cria tcc/tcc_test + usuario tcc_app
copy .env.example .env                            # preencha DATABASE_URL e JWT_SECRET
cargo run                                         # aplica migrations e escuta em BIND_ADDR
cargo test                                        # gate do Principio IX (18 testes)
```

### Frontend
```powershell
cd C:\y\frontend
copy .env.example .env       # VITE_BACKEND_URL=http://localhost:8080
npm install
npm run dev
```
Sem `VITE_BACKEND_URL` o dashboard roda com dados MOCK.

### Firmware
```powershell
# O .env fica na RAIZ do workspace (C:\y\.env), nao em firmware/:
#   WIFI_SSID / WIFI_PASSWORD / BACKEND_URL
# O dotenv!() e uma proc-macro e roda com cwd na raiz do workspace.
cd C:\y\firmware
cargo build                  # toolchain esp ja fixada em rust-toolchain.toml
cargo run                    # flash + monitor
```

### Solver (host)
```powershell
cd C:\y\tomada-solver
cargo test
```

## Notas de build do embarcado

- `sdkconfig.defaults` aponta `CONFIG_PARTITION_TABLE_CUSTOM_FILENAME` para o `partitions.csv`
  deste crate (caminho absoluto Windows).
- `Cargo.toml` raiz força `serde_json` e `minilp` em `opt-level = 0` — bug do backend LLVM
  Xtensa em constant pools `[2 x float]`, que só dispara com opt-level ≥ 1.
- `[profile.dev] opt-level = 0` pelo mesmo motivo; o binário cabe na partição factory de 3.125 MB.
- `codegen-units = 1`: no Windows o default (256) gera ~257 `.o` e estoura o limite de linha de
  comando do linker (`os error 206`).
- Stack da main task em 40 KB — necessário para Wi-Fi/lwIP + alocações Rust.
- LittleFS via componente `joltwallet/littlefs` v1.21.1.

## Estado

Features **004** (backend próprio) e **005** (offline-first + pareamento local) implementadas e
verdes no host: solver 63/63, backend 4/4 (integração contra MySQL), firmware compila para Xtensa
sem warnings, frontend typecheck limpo.

O gate de **hardware** continua **aberto** — flash em placa real, smoke de runtime e
hardware-in-the-loop, incluindo os cenários novos da 005 (boot 100% offline, retomada após queda
de energia, modo sem nuvem, pareamento por botão + PIN entre duas placas e rotação de chave na
revogação). Roteiro em `specs/005-offline-pareamento-local/quickstart.md`.
