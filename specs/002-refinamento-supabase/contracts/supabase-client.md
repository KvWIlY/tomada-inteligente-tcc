# Contract — Cliente Supabase do Firmware (sob JWT por-dispositivo)

Feature: 002-refinamento-supabase | Constituição **VI** (fonte única de verdade) e **VIII**
(segurança). Contratos autoritativos referenciados:
`specs/001-otimizacao-tomadas-milp/contracts/supabase-schema.sql` e `provisioning-api.md`.

Define **como** `firmware/src/comunicacao/supabase.rs` consome o PostgREST após o
refinamento. Não redefine o schema nem a Edge Function — apenas o comportamento do cliente.

## Regras transversais (todas as operações)

- **Headers**: `Authorization: Bearer <jwt>` + `Content-Type: application/json`
  (+ `Prefer` conforme a operação). **NUNCA** enviar `apikey` nem anon key.
- **JWT**: obtido por `provisionamento::obter_jwt_valido()` (renova por margem antes de
  `jwt_exp`). Em `401/403`: renovar uma vez e repetir a operação; nova falha ⇒ registrar
  evento e retornar erro (modo offline), sem travar.
- **TLS**: cert bundle global do ESP-IDF (`use_global_ca_store=true`,
  `crt_bundle_attach`); timeout 10 s (inalterado).
- **`f32`**: serialização/parse manual onde houver float (bug LLVM Xtensa); inteiros e
  strings podem usar `format!`/`serde_json` conforme já feito.
- **Identificadores**: `tomada_id`/`residencia_id` vêm de `CredenciaisLocais`, não de
  constantes.

## Operações de escrita (role `device` — só colunas permitidas)

| Função | Método/Rota | Corpo | `Prefer` |
|--------|-------------|-------|----------|
| `sincronizar_medicoes(lote)` | `POST /rest/v1/medicoes` | **array** JSON de medições pendentes | `return=minimal, resolution=ignore-duplicates` |
| `sincronizar_eventos(lote)` | `POST /rest/v1/eventos` | array JSON de eventos | `return=minimal` (idempotência por chave natural quando aplicável) |
| `atualizar_estado(estado)` | `PATCH /rest/v1/tomadas?id=eq.<tomada_id>` | `{"estado":"…"}` | `return=minimal` |
| `atualizar_calibracao(cstd,con)` | `PATCH …?id=eq.<tomada_id>` | `{"cstd":…,"con":…}` | `return=minimal` |
| `atualizar_telemetria(p,ts)` | `PATCH …?id=eq.<tomada_id>` | `{"p_medida_atual":…,"ultimo_uso_ativo_ts":…}` | `return=minimal` |
| `publicar_config(config)` | `PATCH …?id=eq.<tomada_id>` | `{"config":{…},"config_version":…}` | `return=minimal` |

> **Proibido ao device** (negado por grant de coluna + RLS): escrever `rele_aberto`,
> `p_max`, `mac`, `residencia_id`. O cliente **não** deve montar payloads com essas
> colunas.

### Contrato de lote (FR-010/011/013)

1. Selecionar pendentes (`confirmado=false`), ordenados por `timestamp`, até `MAX_LOTE`.
2. Enviar **uma** requisição com o array.
3. Em 2xx: marcar **todos** os enviados como `confirmado=true` numa **única** consolidação
   na flash (não 1 escrita por registro).
4. Em erro: não marcar; tentar no próximo ciclo (dados preservados).
5. Reenvio é seguro: `resolution=ignore-duplicates` casa no `unique (tomada_id, timestamp)`.

## Operações de leitura

| Função | Método/Rota | Retorno |
|--------|-------------|---------|
| `buscar_desired_state()` | `GET /rest/v1/tomadas?id=eq.<tomada_id>&select=config,config_version,rele_aberto` | `(config, config_version, rele_aberto)` |
| `buscar_todas_tomadas()` | `GET /rest/v1/tomadas?residencia_id=eq.<residencia_id>&select=id,config,cstd,con,p_medida_atual,ultimo_uso_ativo_ts` | lista p/ fallback multi-nó pela nuvem |

### Contrato de pull (FR-015/016)

- Adotar `config` remota **sse** `config_version_remota > config_version_local`; então
  gravar `config_version = remota`. Caso contrário, **no-op**.
- `rele_aberto` é entregue ao **ciclo de decisão** como entrada de comando manual; sua
  aplicação é **subordinada ao `P_max`** (FR-016) — nunca aplicado direto no atuador se
  violar o teto.
- `buscar_todas_tomadas` lê só a própria residência (RLS); resposta grande é processada em
  streaming/buffer limitado, degradando para os dados disponíveis sem estourar memória.

## Erros e observabilidade (FR-017)

| Situação | Ação do cliente | Evento (`eventos.tipo`) |
|----------|------------------|-------------------------|
| `401/403` após 1 retry de renovação | erro → offline | `solver`/`auth_falha` (severidade `warn`) |
| credencial revogada (`renovar` 401) | para escrita na nuvem; segue local | `auth_revogada` (`erro`) |
| `404`/tabela ausente (schema não aplicado) | erro observável; sem corromper local | `schema_ausente` (`erro`) |
| 5xx/timeout | mantém pendentes; retry no ciclo | (log; evento só se persistente) |

> Eventos **nunca** incluem `device_secret`, `jwt` ou claims sensíveis.

## Funções puras testáveis no host (Princípio VII)

- `montar_corpo_lote(&[Medicao]) -> String` (array JSON, escape, `f32` manual).
- `selecionar_pendentes(&[Medicao], max) -> Vec<idx>` (ordenado por `timestamp`).
- `deve_adotar_config(versao_remota, versao_local) -> bool`.
- `deve_renovar(jwt_exp, agora, margem) -> bool`.

Estas não dependem de `esp-idf-svc` e são cobertas por `cargo test` no host.

## Correções pós-integração real (tcc_dashboard, 2026-05-31)

Validado ponta-a-ponta contra o projeto Supabase real (`verify-rls.sh` = 15/15). Estes
pontos **corrigem/precisam o contrato** acima e são obrigatórios no cliente do firmware:

1. **Header `apikey` é OBRIGATÓRIO** (revisa a regra "NUNCA enviar apikey"). O gateway do
   Supabase hospedado **exige** `apikey: <publishable key>` em toda requisição ao
   `/rest/v1/*`, senão responde `401 "No API key found in request"` antes da RLS. A
   **publishable key é PÚBLICA** (`sb_publishable_…`), feita para ficar no cliente, e **não
   concede acesso a nada sozinha** (RLS default-deny). Não fere o Princípio VIII: o segredo/
   identidade continua sendo só o **JWT por-device**. → o firmware leva a publishable key
   (pública) em `config_nuvem`/cache, além do JWT. Headers reais:
   `apikey: <publishable>` + `Authorization: Bearer <jwt device>` + `Content-Type`.
2. **Idempotência exige `on_conflict` na chave natural** (precisa FR-013). `Prefer:
   resolution=ignore-duplicates` sozinho faz `ON CONFLICT` na **PK `id`** (nunca colide) →
   reenvio dá `409 23505`. O correto é
   `POST /rest/v1/medicoes?on_conflict=tomada_id,timestamp` **+** `Prefer:
   return=minimal,resolution=ignore-duplicates`.
3. **Auth = JWT ES256 (signing key assimétrica), não HS256.** O projeto usa JWT Signing
   Keys ECC P-256; a Edge Function `provisionar` assina ES256 com uma chave própria
   importada no projeto (parte pública no JWKS, JWK privada no secret `DEVICE_JWT_JWK`). O
   PostgREST verifica via JWKS.
4. **Grants do role `device`:** além do INSERT, o `device` precisa de **SELECT** em
   `medicoes`/`eventos` (a 001 definiu as *policies* de select mas faltou o grant; sem ele o
   PostgREST nega até o INSERT). A RLS continua filtrando por residência.
