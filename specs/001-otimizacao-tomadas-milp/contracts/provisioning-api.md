# Contract — Provisionamento e Autenticação por Dispositivo (Edge Function)

Feature: 001-otimizacao-tomadas-milp | Constituição **Princípio VIII** (Segurança e
Isolamento Multi-Inquilino) e **Princípio VI** (identidade MAC→UUID).

Define como cada ESP32 obtém **identidade** (MAC→`tomada_id`) e **credenciais**
(segredo durável + JWT de regime) **sem** que qualquer segredo de assinatura viva no
firmware. Os endpoints rodam numa **Supabase Edge Function** (Deno) com acesso
`service_role` — o único ponto que assina JWT e lê `device_credentials`.

## Princípios aplicados (não-negociáveis)

- **MAC identifica, não autentica.** O MAC viaja em claro (Wi-Fi/ESP-NOW); por si só
  nunca concede acesso. A autenticação usa o **segredo durável** por-dispositivo.
- **Segredo de assinatura do JWT só no servidor.** A Edge Function usa
  `SUPABASE_JWT_SECRET` (ou uma signing key dedicada) para assinar. O firmware nunca o vê.
- **`service_role` só no servidor.** Nunca no `.bin` nem no dashboard cliente.
- **`.bin` genérico.** Nenhuma credencial por-dispositivo embutida em compile-time.
- **Armazena só hash.** `device_credentials.secret_hash` e `provisioning_claims.code_hash`
  guardam `sha256(segredo)` — nunca o texto. O texto do segredo é mostrado **uma única
  vez** ao device, na resposta de `provisionar`.

## Visão geral do fluxo

```
                 (1) provisionar (1x por chip)
  ESP32 (AP) ───────────────────────────────────► Edge Function ──► Postgres (service_role)
   |  mac + claim_code                              valida claim,
   |                                                upsert tomada by mac,
   |  ◄──────────────────────────────────────────  cria device_credential (hash),
   |  { tomada_id, residencia_id, device_secret,    assina 1º JWT
   |    jwt, jwt_exp }
   |  grava {tomada_id, residencia_id, device_secret} em LittleFS
   |
   |              (2) renovar (quando jwt perto de expirar)
   └──────────────────────────────────────────────► Edge Function
      tomada_id + device_secret                      confere hash,
                                                      assina novo JWT
      ◄──────────────────────────────────────────── { jwt, jwt_exp }

       (3) regime: device ──Bearer <jwt>──► PostgREST (/rest/v1/...) com RLS por claim
```

## Claims do JWT emitido (role `device`)

```json
{
  "role": "device",                  // PostgREST faz SET ROLE device (least privilege)
  "sub": "<tomada_id uuid>",
  "tomada_id": "<tomada_id uuid>",   // usado pelas policies RLS de escrita do próprio nó
  "residencia_id": "<residencia_id uuid>",
  "iat": <epoch>,
  "exp": <epoch + TTL>               // TTL curto: recomendado 1h..24h
}
```

- Assinado **HS256** com `SUPABASE_JWT_SECRET` (compatível com o `auth.jwt()` do RLS).
- `aud` = `authenticated` (aceito pelo GoTrue/PostgREST). O isolamento real vem das
  policies que chaveiam em `tomada_id`/`residencia_id`, não do `aud`.

## Endpoint 1 — `POST /functions/v1/provisionar`

Registra/vincula o dispositivo pela 1ª vez e emite o segredo durável + 1º JWT.

**Request**
```json
{ "mac": "AA:BB:CC:DD:EE:FF", "claim_code": "<código do dashboard>" }
```

**Lógica server-side (service_role)**
1. `claim ← select * from provisioning_claims where code_hash = sha256(claim_code)`.
2. Rejeitar se `claim` inexistente, `usado_em is not null`, ou `now() > expira_em`.
3. `upsert tomadas (mac, residencia_id) values (mac, claim.residencia_id)
   on conflict (mac) do update set residencia_id = excluded.residencia_id` → obtém `tomada_id`.
4. Gerar `device_secret` aleatório (≥ 32 bytes CSPRNG); `upsert device_credentials
   (tomada_id, secret_hash = sha256(device_secret), revoked_at = null)`.
5. `update provisioning_claims set usado_em = now() where id = claim.id` (uso único).
6. Assinar JWT (claims acima) com TTL.

**Response 200**
```json
{
  "tomada_id": "<uuid>",
  "residencia_id": "<uuid>",
  "device_secret": "<texto, mostrado só aqui>",
  "residencia_key": "<32 bytes hex>",
  "jwt": "<jwt>",
  "jwt_exp": <epoch>
}
```
O firmware grava `tomada_id`, `residencia_id`, `device_secret` e `residencia_key` em
**LittleFS** (`persistencia::credenciais`). O `jwt` fica em RAM (renovável).

> `residencia_key` (passo 6 da lógica): chave HMAC **compartilhada por todos os devices da
> residência**, usada para autenticar os heartbeats ESP-NOW (ver `espnow-protocol.md` §v3).
> É gerada *lazy* na 1ª provisão da casa e reusada nas seguintes (`residencia_secrets`,
> tabela default-deny — **nunca** exposta pela Data API). Entregue só aqui; rotaciona
> apagando a linha e re-provisionando os devices da residência.

**Erros**: `400` payload inválido · `401` claim inválido/expirado/usado · `409`
conflito de MAC sob outra residência (política a definir) · `429` rate-limit.

## Endpoint 2 — `POST /functions/v1/renovar`

Troca o segredo durável por um JWT novo quando o atual está perto de expirar.

**Request**
```json
{ "tomada_id": "<uuid>", "device_secret": "<texto>" }
```

**Lógica server-side**
1. `cred ← select * from device_credentials where tomada_id = $1`.
2. Rejeitar se inexistente, `revoked_at is not null`, ou
   `sha256(device_secret) <> cred.secret_hash` (comparação em tempo constante).
3. `update device_credentials set ultima_renovacao = now()`.
4. Assinar e devolver novo JWT.

**Response 200**: `{ "jwt": "<jwt>", "jwt_exp": <epoch> }`
**Erros**: `401` segredo inválido/revogado · `404` tomada desconhecida · `429` rate-limit.

## Rotação / revogação (sem reflash)

- **Revogar** um device: `update device_credentials set revoked_at = now()` → `renovar`
  passa a falhar; o JWT vigente expira em ≤ TTL.
- **Rotacionar** o segredo: re-provisionar (gera novo `device_secret`) — o antigo deixa
  de validar.

## Segurança de transporte

- Todas as chamadas sobre **HTTPS/TLS** (cert bundle global do ESP-IDF,
  `CONFIG_MBEDTLS_CERTIFICATE_BUNDLE=y`).
- **Rate-limit** por MAC/IP em `provisionar` e `renovar` (mitiga brute-force de
  claim/segredo).
- Claim-codes de **uso único** e curta expiração (ex.: 10 min).

## Impacto no firmware (para `/tasks` e `/implement`)

| Arquivo | Mudança |
|---------|---------|
| `firmware/src/comunicacao/wifi_provisioning.rs` | Coletar `claim_code` no portal AP; chamar `provisionar`; persistir credenciais. |
| `firmware/src/persistencia/` (novo `credenciais.rs`) | Ler/gravar `{tomada_id, residencia_id, device_secret}` + cache do JWT em LittleFS. |
| `firmware/src/comunicacao/supabase.rs` (do `.template`) | Header `Authorization: Bearer <jwt>` (não anon key); renovar via `renovar` em `401`/perto do `exp`. |
| `firmware/src/config_nuvem.rs` | Manter só `SUPABASE_URL` e Wi-Fi de bootstrap; **remover** `ANON_KEY`/`TOMADA_ID`/`RESIDENCIA_ID` por-dispositivo do `.env`. |
| Supabase (servidor) | Edge Function `provisionar`/`renovar`; secrets: `SUPABASE_JWT_SECRET`, `SERVICE_ROLE_KEY`. |

## Obtenção do MAC no ESP32

`esp_efuse_mac_get_default()` (via `esp-idf-sys`) → 6 bytes estáveis de fábrica.
Formatar como `AA:BB:CC:DD:EE:FF`. É o mesmo MAC base do Wi-Fi STA; serve de chave
`tomadas.mac` e de `tomada_id` curto (0..N-1) no ESP-NOW por mapeamento estável.
