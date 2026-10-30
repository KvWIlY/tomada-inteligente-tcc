# `firmware/` - o nó ESP32

Rust sobre `esp-idf` (std), alvo **Xtensa**. Cada tomada mede tensão e corrente, calcula a
**potência ativa**, decide o próprio estado resolvendo um **MILP local**, coordena-se com as
irmãs por **ESP-NOW** e sincroniza com o backend da LAN.

O binário é **genérico**: não há chave, id de dispositivo nem claim-code dentro do `.bin`. A
identidade nasce no primeiro boot, pelo Portal AP.

## Compilar e gravar

Pré-requisitos: o repositório em **`C:\y`** e a toolchain `esp` (`espup`), o `ldproxy` e o
`espflash` instalados. Detalhes e motivos no [README principal](../README.md#pré-requisitos).

```powershell
cd C:\y\firmware
cargo build     # toolchain `esp` já fixada em rust-toolchain.toml
cargo run       # flash + monitor serial
```

**Sempre de dentro de `firmware/`.** O alvo Xtensa está em `firmware/.cargo/config.toml`; da
raiz do repositório o cargo usa o alvo do host e falha com *"Unsupported target"*.

O `cargo run` chama `espflash flash --monitor --partition-table partitions.csv`. O
`--partition-table` **não é opcional**: sem ele o espflash usa a tabela default
(nvs/phy_init/factory), a partição `storage` nunca é gravada e o
`esp_vfs_littlefs_register` falha com o erro 261.

## Configuração - `.env` na RAIZ do workspace

Copie o `.env.example` da raiz do repositório para `C:\y\.env` e preencha:

```ini
# C:\y\.env  - NÃO firmware/.env
WIFI_SSID=...
WIFI_PASSWORD=...
BACKEND_URL=http://192.168.x.x:8080   # sem barra final
```

O `dotenv!()` é uma **proc-macro**: roda em tempo de compilação, com o *cwd* na raiz do
workspace. Um `.env` dentro de `firmware/` é silenciosamente ignorado.

Esses valores são só o *bootstrap*. O que o portal AP grava na NVS tem **prioridade** sobre eles,
então um IP de backend que mudou no DHCP se corrige pelo portal, sem recompilar.

## Pinagem

| Função | GPIO | Detalhe |
|---|---|---|
| Tensão (ZMPT101B) | **32** | ADC1, atenuação `DB_12` (~0-3,1 V) |
| Corrente (SCT-013 20A/1V) | **35** | ADC1, atenuação `DB_12` |
| Relé | **26** | **Opção A**: `LOW` = aberto (OFF), `HIGH` = fechado (STD/ON). Inicia `LOW` |
| Botão BOOT | **0** | Pull-up interno. Abre a janela de **pareamento** de 60 s |
| Botão "usar agora" | **27** | Pull-up interno. **Religa** por 30 min o que o MILP cortou |

São **dois botões** porque são intenções diferentes, e o dono não deveria ter que decorar toque
curto/longo. O GPIO27 é a única porta de religar no **modo standalone**, onde não há dashboard:
sem ele, uma tomada que cortou a TV numa casa sem nuvem não teria como voltar - o controle
remoto do aparelho também está sem energia.

## Boot

```
LittleFS → identidade/PIN → Wi-Fi (portal AP se preciso) → SNTP não-bloqueante → ADC/relé/ESP-NOW
```

**Nada no boot espera internet.** O SNTP é assíncrono e a tomada entra no ciclo sem hora certa.

## O ciclo, a cada `CICLO_S = 10 s`

```
medir (P ativa + FP) → heartbeats ESP-NOW → decidir (MILP) → atuar relé
```

A cada `CICLOS_POR_NUVEM = 6` ciclos (~60 s, só no modo nuvem): bufferiza a medição,
`GET /config` + `POST /telemetria` em lote.

Cada ciclo loga `[DIAG]` (mV no ADC), `[CICLO]` (V, I, P, FP, estado) e `[MEM]` (*heap* livre,
*heap* mínimo e pilha livre mínima da tarefa principal, que tem 40 KB).

O firmware **não chama `esp_wifi_set_ps`**: conectado ao roteador, o rádio fica no
*modem-sleep* padrão do ESP-IDF, que economiza energia mas pode perder pacotes ESP-NOW entre os
sinais do roteador. A coordenação ainda não foi testada nesse modo.

| Constante | Valor | Papel |
|---|---|---|
| `CICLO_S` | 10 s | Cadência da decisão (SC-008) |
| `CICLOS_POR_NUVEM` | 6 | ~1 medição por minuto no banco |
| `TTL_PEER_S` | 30 s | Peer ESP-NOW sem heartbeat some - em **uptime**, não epoch |
| `JANELA_FRESCOR_S` | 300 s | Validade do fallback de coordenação pela nuvem |
| `MAX_FALHAS_SERVIDOR` | 10 | ~2,5 min de falha antes de reabrir o portal |

## Medição - potência ATIVA, não aparente

`sensores::medir` coleta 1000 amostras **intercaladas** de V e I (200 µs ⇒ janela de ~200 ms):

- **`p`** - potência **ATIVA** (W) = `media((v−v_dc)·(i−i_dc)) · fator_v · fator_i`. É o
  `p_medida` que atravessa MILP, calibração, heartbeat e banco.
- **`fp`** - fator de potência `P/S` em [0,1]. **Persistido**.
- `vrms`/`irms` - RMS centrado, removem o bias DC do sensor. **Não são persistidos.**
- `s` - aparente (VA), só existe para derivar o `fp` e para o log.

Guarda-se o FP (um `f32`) e **não** os dois RMS: o FP não é recuperável a partir de `Vrms` e
`Irms`, porque precisa da fase, que só existe durante a janela de amostragem. Um float em vez de
dois, e o dado que importa.

> ⚠️ **A amostragem PRECISA continuar intercalada** - um `v` e um `i` por iteração do mesmo
> `for`. `potencia_ativa` multiplica `v_k · i_k` amostra a amostra e depende da relação de fase.
> Trocar por dois laços separados destrói a fase e o watt vira ruído - mas o RMS de cada canal
> sobrevive (não depende de fase), então **a quebra é silenciosa**.

> ⚠️ **Os fatores de calibração são PLACEHOLDER**: `zmpt101b::FATOR_TENSAO = 0.314` e
> `sct013::FATOR_CORRENTE = 0.02`. A potência ativa está estruturalmente correta, mas a **escala
> absoluta** só fecha depois de calibrar contra multímetro na placa.

## O solver (`src/solver/`)

Lógica **pura** - sem I/O, sem `esp-idf` - mantida assim por disciplina: o
[`simulador/`](../README.md#simulador) compila estes mesmos arquivos (e `decisao/ciclo.rs`,
`sensores/potencia.rs` e `persistencia/config.rs`) num alvo de host, por `#[path]`. Um
`use esp_idf_*` em qualquer um deles quebra o simulador; mudou um deles, compile os dois.

| Módulo | Papel |
|---|---|
| `modelo` / `solver` / `tipos` | Modelo R1-R5 com folgas `y_uso`/`y_age`, resolvido por **busca exata** |
| `portal` | Parser/validador do formulário do AP (framing por `Content-Length`, cap 2048 B) |
| `uso_agora` | Janela temporária de "usar agora" (30 min, uptime monotônico) |
| `retencao` | Escolha de baldes a purgar (confirmados primeiro, nunca o dia corrente) |
| `pareamento` | Máquina do handshake SPAKE2 |
| `replay` | Aceitação de heartbeat por `(boot_id, contador)` |
| `relogio` | Relógio de uptime + offset SNTP |
| `coordenacao` | Soma das irmãs como `carga_fixa` (fallback de nuvem) |
| `sync` | Lote/confirmação idempotente, renovação de JWT, adoção de config |

**Por que dá para resolver exatamente.** Com a R3b (`x_std ≤ 1 - U_i` e `x_on ≤ U_i`: em uso só
ON, ociosa só STD) e a R4 (ociosa há mais de `t_limite` não fica em STD), cada tomada tem no
máximo duas opções, manter ou desligar, e a R2 (teto de potência) é a única restrição que liga as
tomadas. O problema de cada ciclo é uma mochila 0/1. O `solver.rs` resolve em duas etapas: se a
opção mais barata de cada tomada cabe no teto, essa é a solução e não há busca (teto folgado);
senão, uma busca em profundidade com poda parte da solução gulosa (a regra de prioridade) e é
limitada a `LIMITE_NOS = 200_000`. A árvore completa tem no máximo `2^(n+1) - 1` nós, então até
16 tomadas a busca é sempre completa.

Garantias do `resolver()`: **sempre factível** (todas em OFF cabe em qualquer teto), **ótimo
exato** enquanto a busca couber no limite de nós e, acima disso, nunca pior que a regra de
prioridade, e **determinístico**, com desempate explícito: entre soluções de mesmo custo fica
ligada a tomada de menor índice, e o `decidir` ordena as tomadas pelo MAC. É isso que faz todos
os nós da casa chegarem à mesma alocação a partir da mesma entrada, sem líder. A busca obteve o
mesmo `Z` da força bruta `3^n` em 4.000 instâncias aleatórias.

O B&B sobre a relaxação LP do `minilp`, usado até 2026-10-27, saiu do firmware junto com a
dependência.

## Modos de operação

**Nuvem** (padrão) - identidade vem do backend por claim-code, telemetria sobe, config desce.

**Standalone** - identidade local (`residencia_id = "local"`), tick de nuvem pulado, coordenação
por ESP-NOW com a chave do pareamento. A promoção para nuvem reescreve o `cred.json` **sem
descartar a identidade local**, e as medições gravadas vão junto.

**"Usar agora" ≠ "sempre ligada"**: a janela é temporária e devolve a decisão ao MILP; "sempre
ligada" é `is_critical` na config, que vira `W_i = M` dentro do modelo. O pedido continua
subordinado ao `P_max`, e, como a tomada com o relé aberto mede 0 W, o "usar agora" pedido com o
relé aberto conta como **uso ativo no próprio ciclo**: o consumo em uso (`c_on`) é conferido
contra o teto antes de o relé fechar. Até 2026-10-29 a tomada religava em STD contando só o
`cstd`, e o excesso só era percebido no ciclo seguinte.

**Tomada ausente**: uma vizinha sem *heartbeat* por `TTL_PEER_S` (30 s) entra na `carga_fixa` com
o último `p_medida` que anunciou, e é esquecida após 300 s. Reservar o `c_on` dela em vez do
último `p_medida` foi testado no simulador e piorou: a tomada que deixava de ouvir as demais
reservava a casa inteira e desligava a si mesma, geladeira inclusive.

## Portal AP e pareamento

O portal é **single-flow**: pede só Wi-Fi + claim-code, com a URL do servidor num
`<details>Avançado` já preenchido. **Não re-adicionar os rádios** nuvem/standalone/parear -
`parse_form` continua aceitando `modo` (default Nuvem), e o código de Standalone/Parear segue na
árvore de propósito.

- O IP do AP é **192.168.71.1** (default do esp-idf-svc), **não** 192.168.4.1.
- **Pareamento**: o botão BOOT abre uma janela de 60 s; o **PIN de 8 caracteres** da tomada
  *existente* é digitado no portal da *nova*. SPAKE2 → HKDF → ChaCha20-Poly1305: a chave da casa
  viaja cifrada e o PIN nunca trafega. **3 PINs errados fecham a janela.**
- A varredura de canais 1-13 só roda com a STA desconectada: `esp_wifi_set_channel` é recusado
  enquanto associado, e isso quebrava o pareamento **em silêncio** numa placa conectada.

## Persistência (LittleFS, partição `storage` de 640 KB)

`littlefs`, `config` (espelha `tomadas.config` + `config_version`), `credenciais` (`cred.json` v2,
com `modo` e o PIN), `medicao` (baldes por dia), `retencao`.

A purga roda **por espaço a cada tick** - dispara com livre < 15%, libera até 30% - e **por idade
a cada 5 dias**. Escolhe do mais antigo para o mais novo, confirmados primeiro, e nunca toca no
balde do dia corrente.

## Armadilhas confirmadas no código

- **`EspDefaultNvsPartition::take()` é SINGLETON.** O boot toma em `comunicacao::wifi` e guarda
  em `NVS_PART`; um segundo `take()` devolve `ESP_ERR_INVALID_STATE`. Use `wifi::nvs_particao()`.
- **`RouterConfiguration::default()` anuncia 8.8.8.8 como DNS do DHCP do AP** - isso matava o
  portal cativo em silêncio: o sequestro de UDP:53 nunca recebia query. Corrigido com
  `swap_netif_ap` + `dns: Some(AP_IP)`, `secondary_dns: None`. O netif trocado **precisa** de
  chave diferente de `WIFI_AP_DEF` (usamos `WIFI_AP_CAPT`): o original ainda existe na criação e
  `esp_netif_new` recusa chave duplicada com `ESP_ERR_INVALID_ARG`.
- **Xtensa não tem `AtomicI64`** (sem CAS de 64 bits) - o offset do SNTP vive num `Mutex<i64>`.
- **`PinDriver::input(pin, Pull::Up)`** recebe o pull como argumento no esp-idf-hal 0.46
  (`set_pull` é privado).
- **`fator_potencia` é `#[serde(default)]` de propósito** - cobre baldes `med_*.txt` gravados por
  firmware antigo. Sem isso, `carregar_pendentes` rejeitaria linha a linha e o buffer nunca
  sincronizaria.

## Build - por que os profiles são assim

Os `[profile.*]` moram no **`Cargo.toml` da raiz**, não aqui, para valerem em todo o workspace.

- **`opt-level = 0` no próprio `y-firmware`.** O bug do backend LLVM Xtensa em constant pools
  `[2 x float]` mora na desserialização de float do `serde_json` - mas o override
  `[profile.*.package.serde_json]` **não** resolve: `deserialize` é genérico e a monomorfização é
  emitida na CGU de **quem instancia**. O símbolo que quebra termina em `_10y_firmware`.
- **`opt-level = "s"` global**, não 0: com 0 em tudo, a imagem não cabe na partição factory
  depois que a 005 trouxe `spake2`/`curve25519-dalek`/`chacha20poly1305`. Aumentar a partição não
  salva - com 4 MB de flash o teto do factory é ~3,87 MB, ainda menor que a imagem.
- **`codegen-units = 1`**: no Windows o default (256) gera ~257 `.o` e estoura o limite da linha
  de comando do linker (`os error 206`).
- **Stack da main task em 40 KB** (`sdkconfig.defaults`) - Wi-Fi/lwIP mais as alocações do Rust.
  No default de 3584 B o LittleFS estoura e corrompe o heap.
- **`ESP_IDF_SYS_ROOT_CRATE` e `ESP_IDF_SDKCONFIG_DEFAULTS`** estão em `.cargo/config.toml`
  porque o workspace da raiz é **virtual**: sem eles o `esp-idf-sys` não acha o root crate,
  ignora em silêncio o `[package.metadata.esp-idf-sys]` (o LittleFS nunca é baixado) e procura o
  `sdkconfig.defaults` em `C:\y`.

Partições (`partitions.csv`, flash de 4 MB): `factory` 0x10000..0x360000 (3,46 MB), `storage`
(LittleFS) 0x360000..0x400000 (640 KB).

## Estrutura

```
src/
├── main.rs           # boot + loop principal
├── config_nuvem.rs   # URL do backend (NVS tem prioridade sobre o .env)
├── comunicacao/      # wifi (dono do NVS), wifi_provisioning (portal AP+STA cativo),
│                     # provisionamento (claim-code → JWT), backend (cliente HTTP),
│                     # espnow (heartbeat v7: 28 B de dados + 16 B de HMAC-SHA256,
│                     #   boot_id crescente na NVS contra repetição), pareamento (SPAKE2), sntp
├── decisao/          # ciclo (monta o SolverInput, autocalibra cstd/con por EMA), atuador
├── persistencia/     # littlefs, config, credenciais, medicao, retencao
├── sensores/         # zmpt101b, sct013, potencia
└── solver/           # lógica PURA - ver acima
```

## Programas de teste em placa (`examples/`)

São *examples* do pacote para herdar o mesmo perfil de compilação sem mudar o binário principal.
Compilam com `cargo build --example <nome>` e gravam com
`espflash flash --monitor --partition-table partitions.csv <elf>`.

| Programa | O que faz |
|---|---|
| `bancada.rs` | Mede, na placa, o tempo de decisão do solver e a pilha usada em várias configurações de tomadas |
| `calibrar.rs` | Fecha o relé e imprime a cada 2 s os mV no ADC e V, I, P e FP com os fatores atuais; o fator novo é o atual vezes (valor do medidor de referência / valor impresso) |
| `espnow_teste.rs` | Três placas no papel de tomadas do Cenário 6 trocando *heartbeats* v7, com falha injetada; opcionalmente conectadas a um roteador (`ESPNOW_TESTE_SSID`, `ESPNOW_TESTE_SENHA` e `ESPNOW_TESTE_DURACAO_S`, lidos só na compilação) |

## Verificação

`cargo build`, sem *warnings*. **Não há testes de unidade.** O modelo é conferido no host pelo
`verificar` do simulador (invariantes, determinismo e regressões plantadas). Em placa, falta:

- calibrar os sensores e ensaiar a tomada com aparelhos reais;
- medir o tempo da busca exata e a memória com Wi-Fi e ESP-NOW ativos (os números antigos do
  `bancada.rs` são do B&B, que saiu);
- testar o ESP-NOW em cômodos diferentes, com roteador e *modem-sleep*. O teste com três placas
  lado a lado, sem roteador, foi feito com a versão anterior do protocolo;
- o pareamento local e o modo sem nuvem nunca rodaram em placa.
