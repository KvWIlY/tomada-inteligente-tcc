# `backend/` - API Actix-Web sobre MySQL

Fonte única de verdade do sistema. Ninguém mais fala com o banco: nem o dashboard, nem o
firmware. Toda leitura e escrita passa por aqui, autenticada por JWT e autorizada
**default-deny**.

É um **projeto cargo independente** - tem o próprio `[workspace]` e **não** é membro do
workspace da raiz, que é do embarcado (toolchain `esp` e workarounds do bug LLVM Xtensa, nada
disso faz sentido para um servidor). Por isso todo comando aqui roda de `C:\y\backend`.

## Subir do zero

```powershell
cd C:\y\backend
# Antes: troque a senha do usuario tcc_app em scripts\bootstrap-mysql.sql
mysql -u root -p < scripts\bootstrap-mysql.sql   # cria o banco `tcc` e o usuario `tcc_app`
copy .env.example .env                            # DATABASE_URL (com a mesma senha) e JWT_SECRET
cargo run                                         # aplica as migrations e escuta em BIND_ADDR
```

**Sem TLS**: o servidor faz `bind` em HTTP puro. Na rede local, o JWT, o código de instalação e a
chave da casa (que vai no `GET /config`) trafegam em claro, protegidos só pela WPA2 da rede. O
firmware aceita `https://` e valida o certificado, então uma implantação real só precisa de um
*proxy* reverso com certificado de uma autoridade reconhecida na frente deste servidor.

As **migrations são aplicadas no boot do servidor**, por `sqlx::migrate!` - não há passo
manual e nenhum outro processo as aplica. Depois de mexer em `migrations/`, rode `cargo run`
uma vez antes de qualquer outra coisa.

> **Nunca edite uma migration ja aplicada.** O `sqlx` guarda um SHA-384 do arquivo inteiro
> em `_sqlx_migrations` e confere a cada boot. Mudar um comentario, ou so o final de linha,
> derruba o servidor com `VersionMismatch(N)`. O `.gitattributes` da raiz fixa
> `backend/migrations/*.sql text eol=lf` para o `core.autocrlf` do Windows nao reintroduzir
> CRLF num checkout. Mudanca de schema entra em migration NOVA.

> **Um `backend.exe` rodando trava `cargo build`** com `os error 5`: o cargo precisa relinkar o
> binário e o Windows não deixa remover um `.exe` em execução. Pare o servidor antes de
> compilar, ou use `cargo check`, que não relinka.

## Configuração (`.env`)

| Variável | Default | Para que serve |
|---|---|---|
| `DATABASE_URL` | - (obrigatória) | `mysql://tcc_app:SENHA@127.0.0.1:3306/tcc` |
| `JWT_SECRET` | - (obrigatória) | Chave HS256 dos tokens. **32 bytes aleatórios de verdade** |
| `BIND_ADDR` | `0.0.0.0:8080` | `0.0.0.0` é o que permite as placas da LAN alcançarem |
| `JWT_TTL_SECS` | `3600` | Validade do token de usuário e de dispositivo |
| `CORS_ORIGINS` | `http://localhost:5173,http://127.0.0.1:5173` | Origens do **dashboard**, sem barra final |

`Config::carregar()` (`src/config.rs`) aborta o boot se `DATABASE_URL` ou `JWT_SECRET` faltarem -
de propósito: um servidor que sobe sem segredo é pior que um que não sobe.

**`CORS_ORIGINS` só afeta o navegador.** O firmware não manda `Origin` (CORS é política de
navegador), então o plano de dados do device ignora essa lista. Ao servir o `frontend/dist` de
outro host da LAN, acrescente a origem dele aqui, ou o dashboard falha em toda requisição com
um erro que *parece* de autenticação.

## Rotas

Registradas em `src/lib.rs` (`rotas_config`), num único lugar.

### Usuário - exige `Authorization: Bearer <JWT de usuário>`

| Método | Rota | O que faz |
|---|---|---|
| `POST` | `/auth/register` | Cria o dono. Senha em Argon2 |
| `POST` | `/auth/login` | Devolve o JWT de usuário |
| `GET`/`POST` | `/residencias` | Lista / cria casa |
| `GET`/`PATCH` | `/residencias/{id}` | Lê / edita (inclui o `P_max` da casa) |
| `POST` | `/residencias/{id}/claim-codes` | Gera claim-code de **uso único** para provisionar |
| `GET` | `/residencias/{id}/tomadas` | Lista as tomadas da casa |
| `PATCH` | `/tomadas/{id}` | Edita nome, `is_critical`, `rele_aberto`, `config` |
| `DELETE` | `/tomadas/{id}` | **Revoga** - ver abaixo |
| `GET` | `/tomadas/{id}/medicoes` | Série histórica |
| `GET` | `/tomadas/{id}/eventos` | Feed de eventos |

### Dispositivo

| Método | Rota | Auth | O que faz |
|---|---|---|---|
| `POST` | `/provisionar` | **pública** | Troca claim-code + MAC por `device_secret` durável + JWT |
| `POST` | `/renovar` | **pública** | Troca o `device_secret` por um JWT novo |
| `POST` | `/telemetria` | JWT de device | Lote de medições + eventos, idempotente |
| `GET` | `/config` | JWT de device | Config da tomada, `residencia_key`, hora do servidor |
| `GET` | `/residencias/{id}/tomadas/consumo` | JWT de device | Consumo das irmãs (fallback de coordenação) |

`/provisionar` e `/renovar` são públicas por necessidade: o dispositivo ainda não tem token.
São, junto com `/auth/*`, exatamente as rotas onde um atacante pode adivinhar um segredo por
tentativa e erro - e por isso as únicas com **rate-limit** (`src/auth/limite.rs`: 10 tentativas
por IP em janela deslizante de 60 s, contador global ao processo, não por worker do Actix).

## Como a autorização funciona

Não há RLS. A garantia é de código, em `src/auth/middleware.rs`: um handler que exige
autenticação recebe `UsuarioAutenticado { user_id }` ou
`DispositivoAutenticado { tomada_id, residencia_id }` como *extractor*. Sem `Bearer` válido a
extração falha com **401 antes do handler rodar** - não existe caminho em que um handler
protegido veja uma requisição anônima. O escopo (dono da casa, ou a tomada do claim) sai do
claim do token, nunca de um parâmetro da URL.

## Banco

Um banco só, `tcc`. Oito tabelas em `migrations/0001_init.sql`:

`usuarios` → `residencias` → `tomadas` → (`medicoes`, `eventos`, `device_credentials`),
mais `provisioning_claims` e `residencia_secrets`.

Domínio: 1 usuário → 1..N casas (hoje uma; **sem** `UNIQUE` em `owner_id`) → 1..N tomadas. O
mínimo de uma tomada é regra da aplicação, não constraint - a casa existe antes da primeira
tomada ser provisionada pelo claim-code.

Dois pontos que o schema não conta sozinho:

- **`tomadas.rele_aberto` é TRI-ESTADO.** `NULL` = automático, o MILP decide; `TRUE` = cortar;
  `FALSE` = "usar agora", uma janela de 30 min que **o próprio firmware devolve a NULL** via
  `liberar_manual`. Um booleano de dois estados não conseguiria expressar "sem opinião".
- **`medicoes.fator_potencia` é NULL-ável** (`0002_fator_potencia.sql`). `NULL` significa
  medição anterior a 2026-08-24, quando `p_medida` ainda era potência **aparente** (VA) e a fase
  não era gravada. **Não há backfill possível**: o FP precisa da fase, que só existe durante a
  janela de amostragem. Nunca trate esse `NULL` como zero.

### Scripts

| Arquivo | Para quê |
|---|---|
| `scripts/bootstrap-mysql.sql` | Cria o banco e a conta `tcc_app`. Rodar **como root**, uma vez, depois de trocar a senha |
| `scripts/limpar-dados.sql` | `TRUNCATE` em tudo, preservando o schema e `_sqlx_migrations` |
| `scripts/popular-casa-demo.sql` | Casa de demonstração para o dashboard |
| `scripts/popular-atividade-otimizador.sql` | Eventos/medições de atividade do otimizador |

A conta de aplicação é **`tcc_app`**, nunca `root`.

## Duas decisões de contrato que o firmware e o dashboard precisam respeitar

- **`POST /telemetria` aceita `snapshot`** - o lote traz o estado atual junto das medições, e a
  confirmação é idempotente: reenviar o mesmo lote não duplica nada. É o que permite o firmware
  bufferizar em LittleFS enquanto está sem rede e despejar tudo depois.
- **`DELETE /tomadas/{id}` revoga, não apaga.** O histórico fica. E, na **mesma transação**,
  **rotaciona a `residencia_key`** da casa: as tomadas legítimas recebem a chave nova no
  `GET /config` seguinte, sem reprovisionar. A revogação é conferida no `GET /config`
  (`rotas/sync.rs`) e no `POST /renovar` (`rotas/provisionar.rs`), então a tomada revogada não
  recebe mais configuração, nem a chave nova, nem um token novo. O `POST /telemetria` **não** a
  confere: com um JWT ainda dentro da validade (`JWT_TTL_SECS`), os dados dela continuam sendo
  aceitos até o token expirar. A operação é idempotente: revogar duas vezes não rotaciona duas
  vezes.

## Estrutura

```
src/
├── main.rs        # boot: carrega Config, aplica migrations, monta o App
├── lib.rs         # rotas_config() - o mapa de rotas, num lugar só
├── config.rs      # .env -> Config
├── erros.rs       # ApiErro -> resposta HTTP (sem vazar detalhe de banco)
├── auth/          # jwt (HS256), senha (Argon2), middleware (extractors), limite (rate-limit)
├── db/            # uma camada por agregado: usuarios, residencias, tomadas, credenciais, telemetria
├── dominio/       # tipos compartilhados entre rotas e db
└── rotas/         # handlers: auth, residencias, tomadas, provisionar, sync
```

`db/` é a única camada que emite SQL; os handlers em `rotas/` não montam query.

Não existe `GET /tomadas/{id}`: o dashboard sempre carrega a lista por casa
(`GET /residencias/{id}/tomadas`) e nunca pediu uma tomada isolada. A rota existia sem
consumidor e foi removida em 2026-08-30.

## Verificação

`cargo check` (rápido, não relinka) e `cargo build`. **Não há testes** automatizados: o único
arquivo de teste que existiu, `tests/rotacao_chave.rs`, foi apagado.
