#!/usr/bin/env bash
# criar-claim.sh — cunha um claim-code de pareamento (provisioning_claims).
#
# O claim-code so pode ser criado com privilegio (service_role / dono). Roda na SUA
# maquina, NUNCA no ESP32 (Principio VIII): gera o codigo, calcula sha256 hex (mesmo hash
# da Edge Function `provisionar`), insere via PostgREST com a service_role key e imprime
# o codigo em texto p/ digitar no portal AP (ou no .env).
#
# Pre-requisitos (variaveis de ambiente):
#   export SUPABASE_URL="https://<proj>.supabase.co"
#   export SUPABASE_SERVICE_ROLE_KEY="<service_role secret>"   # Project Settings > API
#
# Uso:
#   ./scripts/criar-claim.sh <residencia_id>                 # codigo aleatorio, 2h
#   ./scripts/criar-claim.sh <residencia_id> MEU-123 6       # codigo, horas
#   ./scripts/criar-claim.sh --listar                        # lista residencias
set -euo pipefail

URL="${SUPABASE_URL:-${PROJECT_URL:-}}"
KEY="${SUPABASE_SERVICE_ROLE_KEY:-${SERVICE_ROLE_KEY:-}}"

if [[ -z "$URL" || -z "$KEY" ]]; then
  echo "Defina SUPABASE_URL e SUPABASE_SERVICE_ROLE_KEY antes de rodar." >&2
  exit 2
fi
URL="${URL%/}"

if [[ "${1:-}" == "--listar" ]]; then
  curl -fsS "$URL/rest/v1/residencias?select=id,nome,p_max" \
    -H "apikey: $KEY" -H "Authorization: Bearer $KEY"
  echo
  exit 0
fi

RESIDENCIA_ID="${1:-}"
if [[ -z "$RESIDENCIA_ID" ]]; then
  echo "Faltou <residencia_id>. Use --listar para ver os ids." >&2
  exit 2
fi

CODIGO="${2:-}"
HORAS="${3:-2}"
if [[ -z "$CODIGO" ]]; then
  SUF=$(LC_ALL=C tr -dc 'A-Z0-9' < /dev/urandom | head -c 6)
  CODIGO="TOMADA-$SUF"
fi

# sha256 hex minusculo (igual ao sha256Hex da Edge Function).
HASH=$(printf '%s' "$CODIGO" | openssl dgst -sha256 -hex | sed 's/^.*= //')
EXPIRA=$(date -u -d "+${HORAS} hours" +"%Y-%m-%dT%H:%M:%SZ" 2>/dev/null \
        || date -u -v+"${HORAS}"H +"%Y-%m-%dT%H:%M:%SZ")

curl -fsS -X POST "$URL/rest/v1/provisioning_claims" \
  -H "apikey: $KEY" -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" -H "Prefer: return=minimal" \
  -d "{\"residencia_id\":\"$RESIDENCIA_ID\",\"code_hash\":\"$HASH\",\"expira_em\":\"$EXPIRA\"}"

echo
echo "Claim-code criado (uso unico, expira em ${HORAS}h):"
echo "  CODIGO     : $CODIGO"
echo "  residencia : $RESIDENCIA_ID"
echo "  expira_em  : $EXPIRA (UTC)"
echo
echo "Digite esse CODIGO no portal AP da tomada (ou em firmware/.env: CLAIM_CODE=$CODIGO)."
