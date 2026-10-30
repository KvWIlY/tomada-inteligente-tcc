# Tomada inteligente - corte do consumo em espera com teto de potência (TCC)

Rede de tomadas ESP32 que corta o consumo em espera (*standby*) dos aparelhos ociosos sem deixar
a casa ultrapassar o limite de potência do circuito. Cada tomada mede tensão e corrente, calcula
a **potência ativa** (média do produto instantâneo, não `Vrms·Irms`), decide o próprio estado
resolvendo um **modelo MILP** local, coordena-se com as demais por **ESP-NOW** e sincroniza com
um **backend próprio** na rede local.

Três estados por tomada: **OFF** (relé aberto), **STD** (energizada, em espera) e **ON** (em uso).

## Os quatro projetos

| Diretório | O quê | Alvo | Detalhes |
|---|---|---|---|
| [`firmware/`](firmware/README.md) | Rust + esp-idf no ESP32: sensores, relé, ESP-NOW, cliente do backend e `src/solver/`, a lógica pura do modelo | Xtensa | [README](firmware/README.md) |
| [`backend/`](backend/README.md) | Rust + Actix-Web sobre MySQL: fonte de verdade, autenticação própria (JWT), autorização *default-deny* | host | [README](backend/README.md) |
| [`frontend/`](frontend/README.md) | Vite + TypeScript + Tailwind 4 + Basecoat UI, sem framework: o *dashboard* do morador | navegador | [README](frontend/README.md) |
| `simulador/` | CLI que roda o modelo **do firmware** no PC sobre uma base de consumo em CSV, compilando os mesmos arquivos de `firmware/src/solver/` | host | seção [Simulador](#simulador) |

Cada projeto se compila de dentro da própria pasta, porque são duas toolchains:

- **`cd firmware && cargo build`**: o alvo Xtensa está em `firmware/.cargo/config.toml`. Da raiz,
  o cargo usa o alvo do host e falha com *"Unsupported target"*.
- **`cd backend && cargo check`** e **`cd simulador && cargo build --release`**: projetos cargo
  **independentes**, cada um com o próprio `[workspace]` e a toolchain `stable`. A raiz fixa a
  toolchain `esp` e *profiles* com contornos para o bug do LLVM Xtensa, que não servem no host.
- **`cd frontend && npm run build`**.

## Como as peças se ligam

```
   ┌──────────────────┐  ESP-NOW (heartbeat autenticado)  ┌──────────────────┐
   │     ESP32 #1     │◄─────────────────────────────────►│     ESP32 #2     │
   │ mede→decide→relé │                                   │ mede→decide→relé │
   └─────────┬────────┘                                   └─────────┬────────┘
             │  POST /telemetria  ·  GET /config                    │
             └──────────────────────────┬───────────────────────────┘
                                        ▼
                              ┌───────────────────┐
                              │   backend (LAN)   │──── MySQL `tcc` (só ele acessa)
                              └─────────┬─────────┘
                                        │  JWT de usuário
                                        ▼
                              ┌───────────────────┐
                              │  dashboard (web)  │
                              └───────────────────┘
```

**A decisão é da tomada, não da nuvem.** A cada 10 s, cada tomada monta o modelo da casa inteira
com as medições anunciadas pelas vizinhas, resolve-o e aplica só o próprio estado. Com as
restrições do modelo, cada tomada tem no máximo duas opções (manter ou desligar) e o teto de
potência é a única restrição que as liga, então o problema é uma mochila 0/1, resolvida por
**busca exata** com poda e desempate determinístico pelo endereço MAC. Como todas resolvem o mesmo
problema da mesma forma, chegam à mesma alocação sem um líder.

O backend é fonte de verdade de **configuração e histórico**, não um controlador: se ele cair, as
tomadas continuam decidindo. Uma tomada que para de transmitir por 30 s entra no cálculo como
carga fixa, com o último consumo que anunciou; se o rádio falhar e houver internet, a carga das
demais vem de `GET /residencias/{id}/tomadas/consumo`.

## Segurança

- **Fonte de verdade**: MySQL, acessível **apenas** pelo backend.
- **Identidade da tomada**: MAC → `tomada_id`. O MAC identifica, não autentica.
- **Autenticação**: JWT HS256 emitido pelo backend, para usuários (*dashboard*) e tomadas.
- **Autorização**: *default-deny* em todo *handler*, pelo *claim* do token.
- **Segredos**: `JWT_SECRET` e a credencial do MySQL vivem **só** em `backend/.env`. O `.bin` do
  firmware é genérico: sem chave de API, sem identificador por tomada, sem código de instalação.
- **Instalação**: o morador gera no *dashboard* um código de uso único e o digita no portal que a
  tomada abre como ponto de acesso; o backend o troca por uma credencial durável e um JWT.
- **ESP-NOW**: cada *heartbeat* leva uma etiqueta HMAC-SHA256 com a chave da casa e um par
  (número de inicialização crescente, contador) contra repetição de pacotes gravados.
- **Revogação**: revogar uma tomada corta o acesso dela à API e rotaciona a chave da casa na mesma
  transação; as demais recebem a chave nova no `GET /config` seguinte.
- **Sem TLS no protótipo**: o backend escuta em HTTP puro na rede local. O firmware aceita
  `https://` e valida o certificado; uma implantação real exige um *proxy* reverso com TLS.

## Operação sem internet

A decisão não depende de internet, de servidor nem de hora certa:

- **Sem servidor**: a tomada decide com a configuração guardada na memória *flash* e continua
  coordenando-se com as demais pelo ESP-NOW. A instalação de uma tomada nova e a rotação da chave
  é que dependem do backend.
- **Relógio**: sem NTP, a tomada opera pelo tempo desde o boot; o agendamento por hora do dia fica
  suspenso até a hora chegar, e os registros pendentes têm o carimbo corrigido.
- **Queda de energia**: se o roteador voltar depois da tomada, o portal continua tentando a rede
  salva a cada 30 s e fecha sozinho quando ela volta.
- **Modo sem nuvem e pareamento local** (SPAKE2 com o PIN de 8 caracteres da tomada) existem no
  firmware, mas o portal da versão atual não os oferece e eles não foram testados em placa.

## Pré-requisitos

### O repositório precisa estar em `C:\y`

Clone o projeto **na raiz do disco, em `C:\y`**, e não dentro de `Documentos` ou de outra pasta
funda. Há dois motivos:

- **Caminho absoluto no build do firmware.** O `firmware/.cargo/config.toml` aponta
  `ESP_IDF_SDKCONFIG_DEFAULTS = "C:/y/firmware/sdkconfig.defaults"`. Em outro caminho, o
  `esp-idf-sys` não encontra esse arquivo e **não avisa**: a pilha da tarefa principal volta ao
  padrão de 3,5 KB em vez de 40 KB, e o firmware corrompe a memória quando o LittleFS sobe. Se o
  projeto tiver mesmo que ficar em outro lugar, troque esse caminho no `config.toml`.
- **Limite de caminho do Windows.** O ESP-IDF é baixado e compilado dentro do repositório, em
  `.embuild/`, com caminhos de pasta muito longos. Numa pasta funda, eles passam do limite de 260
  caracteres do Windows e o build falha com erros que não dizem isso.

```powershell
git clone <url-do-repositorio> C:\y
```

### Ferramentas

| Ferramenta | Para quê | Como instalar |
|---|---|---|
| **Rust** (`rustup`, toolchain `stable`) | backend e simulador | [rustup.rs](https://rustup.rs); no Windows, com as Build Tools do Visual Studio (C++) |
| **`espup`** e a toolchain **`esp`** | firmware: Rust para Xtensa (ESP32) | `cargo install espup` e depois `espup install` |
| **`ldproxy`** | firmware: *linker* do `esp-idf-sys` | `cargo install ldproxy` |
| **`espflash`** (4.x) | firmware: gravar a placa e abrir o monitor serial | `cargo install espflash` |
| **Python 3** e **Git** | firmware: o `esp-idf-sys` baixa o ESP-IDF v5.2.3 e monta o ambiente Python dele | [python.org](https://www.python.org), [git-scm.com](https://git-scm.com) |
| **Node.js** (com npm) | frontend | [nodejs.org](https://nodejs.org) |
| **MySQL 8** | backend | [dev.mysql.com](https://dev.mysql.com/downloads/) |
| **Driver USB-serial** da placa (CP210x ou CH340) | gravar o ESP32 | site do fabricante do chip da placa |

A toolchain do firmware é escolhida pelo `firmware/rust-toolchain.toml` (`channel = "esp"`); a do
backend e do simulador, pelo `stable`. Por isso cada projeto é compilado de dentro da própria
pasta.

**O primeiro `cargo build` do firmware demora:** ele baixa o ESP-IDF v5.2.3 e as ferramentas da
Espressif para `C:\y\.embuild\` (alguns GB, ignorados pelo git) e compila tudo. Os builds
seguintes levam cerca de 1 minuto.

Versões usadas no desenvolvimento: Rust 1.95 (`stable` e `esp`), `espup` 0.17, `espflash` 4.3,
ESP-IDF v5.2.3, Node.js 24, Python 3.13.

## Subir o sistema inteiro

Na ordem: o *dashboard* precisa do backend, e a tomada precisa dos dois para ser instalada.

```powershell
# 1. Backend  (detalhes em backend/README.md)
cd C:\y\backend
#    Troque a senha do usuario tcc_app em scripts\bootstrap-mysql.sql antes de rodar,
#    e use a mesma no DATABASE_URL do .env
mysql -u root -p < scripts\bootstrap-mysql.sql
copy .env.example .env      # DATABASE_URL + JWT_SECRET
cargo run                   # aplica as migrations no boot e escuta em :8080

# 2. Dashboard  (detalhes em frontend/README.md)
cd C:\y\frontend
copy .env.example .env      # VITE_BACKEND_URL=http://localhost:8080
npm install
npm run dev                 # :5173

# 3. Firmware  (detalhes em firmware/README.md)
#    O .env fica na RAIZ (C:\y\.env), não em firmware/: o dotenv!() é proc-macro
#    e roda com o diretório da raiz do workspace.
cd C:\y
copy .env.example .env      # opcional: Wi-Fi e BACKEND_URL de desenvolvimento
cd C:\y\firmware
cargo build
cargo run                   # grava e abre o monitor serial
```

Depois: cadastre o morador no *dashboard*, crie a casa, gere um código de instalação e digite-o no
portal da tomada (rede `Tomada-XXXX`, `http://192.168.71.1`).

> **Um `backend.exe` ou `simulador.exe` rodando trava o `cargo build`** com `os error 5`: o Windows
> não deixa substituir um `.exe` em execução. Pare o programa, ou use `cargo check`.

## Simulador

Roda o modelo do firmware no PC, sem placa: a cada passo de 10 s, cada tomada simulada chama o
mesmo `Ciclo::decidir` do firmware e vê as vizinhas pelo anúncio do passo anterior, como na rede
ESP-NOW. Os arquivos de `firmware/src/solver/`, `decisao/ciclo.rs`, `sensores/potencia.rs` e
`persistencia/config.rs` são incluídos por `#[path]`, sem cópia; por isso eles não podem usar
nada de `esp_idf_*`.

```powershell
cd C:\y\simulador
cargo build --release
.\target\release\simulador.exe gerar-base caso.toml                 # base CSV a partir do caso
.\target\release\simulador.exe rodar --realimentar --rastro caso.toml
.\target\release\simulador.exe estimar caso.toml                    # projeta o tempo da rodada
```

Cada rodada grava uma pasta em `simulador/rodadas/` (ignorada pelo git) com o resumo, o rastro
de decisões e o *hash* dos fontes do firmware usados. O caso (`.toml`) também permite trocar o
modelo pela regra fixa, pela regra de prioridade ou por uma execução centralizada, simular perda
de pacotes ESP-NOW e um morador que demora a religar os aparelhos.

## Estado

O que está pronto e verificado por compilação: firmware para Xtensa, backend contra o MySQL,
*dashboard* e simulador. O modelo foi avaliado no simulador em seis cenários residenciais e em
seis residências da base pública REFIT, e a coordenação por ESP-NOW foi testada em três placas,
lado a lado e sem roteador.

**Em aberto:**

- **Bancada com aparelhos reais**: o protótipo ainda não mediu nem cortou um aparelho real. Os
  fatores de calibração de tensão e corrente são provisórios e só fecham contra um medidor de
  referência.
- **Consumo da própria tomada**: estimado pelas folhas de dados; falta medir.
- **Tempo de decisão e memória no ESP32**: a busca exata ainda não foi medida na placa.
- **ESP-NOW dentro de casa**: falta o teste com as placas em cômodos diferentes, ligadas ao
  roteador e com o rádio em *modem-sleep*.
- **Testes automatizados**: não há suíte de testes; além da compilação, só o `verificar` do
  simulador confere o modelo sobre casos declarados.
