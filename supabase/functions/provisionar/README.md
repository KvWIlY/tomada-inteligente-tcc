# Edge Function `provisionar`

Provisionamento e renovação de credenciais por-dispositivo (Princípio VIII).
Contrato completo: `specs/001-otimizacao-tomadas-milp/contracts/provisioning-api.md`.

## Endpoints (POST)

- `/functions/v1/provisionar` → `provisionar({ mac, claim_code })`
- `/functions/v1/provisionar/renovar` → `renovar({ tomada_id, device_secret })`

## Secrets (obrigatórios)

> O projeto usa **JWT Signing Keys assimétricas (ES256 / ECC P-256)**. O PostgREST verifica
> os tokens contra o JWKS do projeto, então a função precisa assinar com uma chave cuja
> parte pública esteja nesse JWKS. A private key do Supabase não é exportável — por isso
> geramos **nossa própria** chave ES256, importamos a parte pública no projeto e guardamos a
> JWK privada como secret.

```bash
# 1. Gerar a chave ES256 (gera uma JWK privada com kty=EC, crv=P-256, d, x, y, kid).
supabase gen signing-key --algorithm ES256

# 2. Importar a MESMA chave no projeto: Dashboard → Authentication → JWT Signing Keys →
#    "Add standby key" → "Import key" → colar a JWK. Ela entra no JWKS (verificação ES256).

# 3. Guardar a JWK privada como secret da função (a função lê 'kid' e assina com ela):
supabase secrets set DEVICE_JWT_JWK='<a JWK privada inteira, em JSON>'
# (SUPABASE_URL e SUPABASE_SERVICE_ROLE_KEY são injetados automaticamente.)
# Opcional: TTL do JWT em segundos (default 3600).
supabase secrets set JWT_TTL_SECONDS=3600
```

## ⚠️ Deploy — desativar verify_jwt

O dispositivo chama esta função **sem** um JWT (é justamente para obter o primeiro).
Portanto a verificação de JWT da plataforma DEVE ser desligada para esta função:

```bash
supabase functions deploy provisionar --no-verify-jwt
```

Ou em `supabase/config.toml`:

```toml
[functions.provisionar]
verify_jwt = false
```

A autenticação real é feita **dentro** da função (claim-code no `provisionar`,
`device_secret` no `renovar`), comparada por hash em tempo constante.

## Teste rápido (após deploy)

```bash
curl -i -X POST "$SUPABASE_URL/functions/v1/provisionar" \
  -H "Content-Type: application/json" \
  -d '{"mac":"AA:BB:CC:DD:EE:FF","claim_code":"<code do dashboard>"}'
```

Espera-se `200` com `{ tomada_id, residencia_id, device_secret, jwt, jwt_exp }`.
