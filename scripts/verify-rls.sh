#!/usr/bin/env bash
# Verificacao de isolamento RLS ponta-a-ponta (feature 002-refinamento-supabase).
#
# Exercita os 15 casos de specs/002-refinamento-supabase/contracts/rls-verification.md
# pelo caminho REAL do cliente (HTTP + Authorization: Bearer <jwt device>), cobrindo
# SC-003/SC-004 e FR-006/007/008/009. Imprime PASS/FAIL por caso e falha o processo
# (exit 1) em qualquer divergencia.
#
# "negado" = HTTP 401/403  OU  2xx afetando 0 linhas (PostgREST + RLS pode devolver
# sucesso vazio para filtros que nao casam a policy). O criterio e: nenhum dado de
# outro inquilino lido/alterado e nenhuma coluna reservada escrita.
#
# Uso:
#   export SUPABASE_URL=https://<proj-teste>.supabase.co
#   export SUPABASE_APIKEY=<publishable key OU anon key>  # PUBLICA: o gateway exige em toda req.
#   export JWT_A=...   JWT_B=...           # JWTs de device (provisionar com claim-codes de teste)
#   export A_T1=<uuid tomada A.t1>  RESID_A=<uuid residencia A>
#   export B_T1=<uuid tomada B.t1>  RESID_B=<uuid residencia B>
#   bash scripts/verify-rls.sh
#
# Nota: o gateway do Supabase exige o header `apikey` (a publishable/anon key, PUBLICA) em
# toda requisicao -- ele so faz roteamento; a auth/isolamento real e o `Authorization:
# Bearer <jwt device>` + RLS. A publishable key sozinha nao da acesso a nada (RLS nega).
# NUNCA usar segredos de producao. Use um projeto Supabase de teste.

set -u

# ---- pre-condicoes ----
faltando=0
for v in SUPABASE_URL SUPABASE_APIKEY JWT_A JWT_B A_T1 RESID_A B_T1 RESID_B; do
  if [ -z "${!v:-}" ]; then
    echo "ERRO: variavel de ambiente '$v' nao definida." >&2
    faltando=1
  fi
done
[ "$faltando" -eq 0 ] || { echo "Defina as variaveis acima e rode de novo." >&2; exit 2; }

BASE="${SUPABASE_URL%/}/rest/v1"
TS="$(date +%s)"   # timestamp compartilhado por #9 e #11 (reenvio idempotente)

PASS=0
FAIL=0

# req METHOD PATH JWT [BODY] [PREFER]
# Ecoa: "<http_code>\n<body>"  (body sem a ultima linha de status)
req() {
  local method="$1" path="$2" jwt="$3" body="${4:-}" prefer="${5:-}"
  # O `apikey` (publishable, publica) vai em TODA requisicao -- exigencia do gateway.
  local args=(-s -o - -w $'\n%{http_code}' -X "$method" -H "apikey: ${SUPABASE_APIKEY}")
  if [ "$jwt" != "ANON" ]; then
    # auth real: JWT por-device (RLS). O caso "anon" manda so o apikey, sem Authorization.
    args+=(-H "Authorization: Bearer ${jwt}")
  fi
  args+=(-H "Content-Type: application/json")
  [ -n "$prefer" ] && args+=(-H "Prefer: ${prefer}")
  [ -n "$body" ]   && args+=(--data "$body")
  curl "${args[@]}" "${BASE}${path}" 2>/dev/null
}

status_of() { printf '%s' "$1" | tail -n1; }
body_of()   { printf '%s' "$1" | sed '$d'; }

is_2xx()      { [ "$1" -ge 200 ] && [ "$1" -le 299 ]; }
is_denied()   { [ "$1" = "401" ] || [ "$1" = "403" ]; }
# array JSON vazio (ou corpo vazio)
is_empty_arr() {
  local b; b="$(printf '%s' "$1" | tr -d '[:space:]')"
  [ -z "$b" ] || [ "$b" = "[]" ]
}

ok()  { PASS=$((PASS+1)); printf '  PASS  #%-2s %s\n' "$1" "$2"; }
bad() { FAIL=$((FAIL+1)); printf '  FAIL  #%-2s %s\n' "$1" "$2"; printf '        -> status=%s body=%s\n' "$3" "$(printf '%s' "$4" | head -c 200)"; }

# ----------------------------------------------------------------------
# Operacoes LEGITIMAS (devem ser permitidas): 1, 2, 5, 9, 11
# ----------------------------------------------------------------------

# #1 A.t1 le a propria tomada -> 200, 1 linha (contendo A_T1)
r=$(req GET "/tomadas?id=eq.${A_T1}&select=id" "$JWT_A"); s=$(status_of "$r"); b=$(body_of "$r")
if is_2xx "$s" && printf '%s' "$b" | grep -q "$A_T1"; then ok 1 "A.t1 GET propria tomada (1 linha)"; else bad 1 "A.t1 GET propria tomada (esperado 1 linha)" "$s" "$b"; fi

# #2 A.t1 le irmas da mesma residencia -> 200, >=1 linha
r=$(req GET "/tomadas?residencia_id=eq.${RESID_A}&select=id" "$JWT_A"); s=$(status_of "$r"); b=$(body_of "$r")
if is_2xx "$s" && ! is_empty_arr "$b"; then ok 2 "A.t1 GET irmas (mesma residencia, >=1 linha)"; else bad 2 "A.t1 GET irmas (esperado >=1 linha)" "$s" "$b"; fi

# #5 A.t1 PATCH coluna permitida (estado) na propria -> 2xx
r=$(req PATCH "/tomadas?id=eq.${A_T1}" "$JWT_A" '{"estado":"ON"}' "return=representation"); s=$(status_of "$r"); b=$(body_of "$r")
if is_2xx "$s" && ! is_empty_arr "$b"; then ok 5 "A.t1 PATCH estado (coluna permitida)"; else bad 5 "A.t1 PATCH estado (esperado 2xx, 1 linha)" "$s" "$b"; fi

# #9 A.t1 POST medicao propria -> 2xx
# on_conflict=tomada_id,timestamp: faz o ON CONFLICT casar na chave NATURAL (nao na PK `id`),
# habilitando a idempotencia de `resolution=ignore-duplicates` (FR-013).
MED_A=$(printf '{"tomada_id":"%s","timestamp":%s,"p_medida":1.00,"estado":"ON","rele_aberto":false}' "$A_T1" "$TS")
r=$(req POST "/medicoes?on_conflict=tomada_id,timestamp" "$JWT_A" "$MED_A" "return=minimal,resolution=ignore-duplicates"); s=$(status_of "$r"); b=$(body_of "$r")
if is_2xx "$s"; then ok 9 "A.t1 POST medicao propria"; else bad 9 "A.t1 POST medicao propria (esperado 2xx)" "$s" "$b"; fi

# #11 A.t1 reenvia a MESMA medicao -> 2xx sem duplicar (ignore-duplicates + on_conflict natural)
r=$(req POST "/medicoes?on_conflict=tomada_id,timestamp" "$JWT_A" "$MED_A" "return=minimal,resolution=ignore-duplicates"); s=$(status_of "$r"); b=$(body_of "$r")
if is_2xx "$s"; then ok 11 "A.t1 reenvio idempotente da medicao"; else bad 11 "A.t1 reenvio idempotente (esperado 2xx)" "$s" "$b"; fi

# ----------------------------------------------------------------------
# Operacoes NEGADAS/VAZIAS: 3, 4, 6, 7, 8, 10, 12, 13, 14, 15
# ----------------------------------------------------------------------

# #3 A.t1 tenta ler tomadas da residencia B -> 200 vazio (RLS filtra)
r=$(req GET "/tomadas?residencia_id=eq.${RESID_B}&select=id" "$JWT_A"); s=$(status_of "$r"); b=$(body_of "$r")
if { is_2xx "$s" && is_empty_arr "$b"; } || is_denied "$s"; then ok 3 "A.t1 GET residencia B negado/vazio"; else bad 3 "A.t1 GET residencia B (esperado vazio)" "$s" "$b"; fi

# #4 A.t1 tenta ler a tomada de B -> 200 vazio
r=$(req GET "/tomadas?id=eq.${B_T1}&select=id" "$JWT_A"); s=$(status_of "$r"); b=$(body_of "$r")
if { is_2xx "$s" && is_empty_arr "$b"; } || is_denied "$s"; then ok 4 "A.t1 GET tomada de B negado/vazio"; else bad 4 "A.t1 GET tomada de B (esperado vazio)" "$s" "$b"; fi

# #6 A.t1 tenta escrever coluna reservada rele_aberto na propria -> negado (grant de coluna)
r=$(req PATCH "/tomadas?id=eq.${A_T1}" "$JWT_A" '{"rele_aberto":true}' "return=representation"); s=$(status_of "$r"); b=$(body_of "$r")
if is_denied "$s" || { is_2xx "$s" && is_empty_arr "$b"; }; then ok 6 "A.t1 PATCH rele_aberto negado"; else bad 6 "A.t1 PATCH rele_aberto (esperado negado)" "$s" "$b"; fi

# #7 A.t1 tenta elevar o teto p_max (coluna de `residencias`) -> negado (device so tem SELECT)
r=$(req PATCH "/residencias?id=eq.${RESID_A}" "$JWT_A" '{"p_max":99}' "return=representation"); s=$(status_of "$r"); b=$(body_of "$r")
if is_denied "$s" || { is_2xx "$s" && is_empty_arr "$b"; }; then ok 7 "A.t1 PATCH residencias.p_max negado"; else bad 7 "A.t1 PATCH p_max (esperado negado)" "$s" "$b"; fi

# #8 A.t1 tenta PATCH na linha de B (coluna permitida) -> negado/0 linhas (with check)
r=$(req PATCH "/tomadas?id=eq.${B_T1}" "$JWT_A" '{"estado":"OFF"}' "return=representation"); s=$(status_of "$r"); b=$(body_of "$r")
if is_denied "$s" || { is_2xx "$s" && is_empty_arr "$b"; }; then ok 8 "A.t1 PATCH linha de B negado/0 linhas"; else bad 8 "A.t1 PATCH linha de B (esperado negado/vazio)" "$s" "$b"; fi

# #10 A.t1 tenta inserir medicao com tomada_id de B -> negado (with check)
MED_B=$(printf '{"tomada_id":"%s","timestamp":%s,"p_medida":1.00,"estado":"ON","rele_aberto":false}' "$B_T1" "$TS")
r=$(req POST "/medicoes" "$JWT_A" "$MED_B" "return=representation"); s=$(status_of "$r"); b=$(body_of "$r")
if is_denied "$s" || { is_2xx "$s" && is_empty_arr "$b"; }; then ok 10 "A.t1 POST medicao de B negado"; else bad 10 "A.t1 POST medicao de B (esperado negado)" "$s" "$b"; fi

# #12 A.t1 tenta ler medicoes da residencia B -> 200 vazio
r=$(req GET "/medicoes?tomada_id=eq.${B_T1}&select=tomada_id" "$JWT_A"); s=$(status_of "$r"); b=$(body_of "$r")
if { is_2xx "$s" && is_empty_arr "$b"; } || is_denied "$s"; then ok 12 "A.t1 GET medicoes de B negado/vazio"; else bad 12 "A.t1 GET medicoes de B (esperado vazio)" "$s" "$b"; fi

# #13 A.t1 tenta ler a tabela de segredos device_credentials -> negado (sem policy p/ device)
r=$(req GET "/device_credentials?select=tomada_id" "$JWT_A"); s=$(status_of "$r"); b=$(body_of "$r")
if is_denied "$s" || { is_2xx "$s" && is_empty_arr "$b"; }; then ok 13 "A.t1 GET device_credentials negado/vazio"; else bad 13 "A.t1 GET device_credentials (esperado negado)" "$s" "$b"; fi

# #14 A.t1 tenta ler provisioning_claims -> negado
r=$(req GET "/provisioning_claims?select=id" "$JWT_A"); s=$(status_of "$r"); b=$(body_of "$r")
if is_denied "$s" || { is_2xx "$s" && is_empty_arr "$b"; }; then ok 14 "A.t1 GET provisioning_claims negado/vazio"; else bad 14 "A.t1 GET provisioning_claims (esperado negado)" "$s" "$b"; fi

# #15 anon (sem Authorization) em qualquer tabela de dados -> negado
r=$(req GET "/tomadas?select=id" "ANON"); s=$(status_of "$r"); b=$(body_of "$r")
if is_denied "$s" || { is_2xx "$s" && is_empty_arr "$b"; }; then ok 15 "anon GET tomadas negado/vazio"; else bad 15 "anon GET tomadas (esperado negado)" "$s" "$b"; fi

# ----------------------------------------------------------------------
echo
echo "Resultado: ${PASS} PASS / ${FAIL} FAIL (de 15 casos)"
if [ "$FAIL" -ne 0 ]; then
  echo "RLS NAO conforme -- ver casos FAIL acima (SC-003/SC-004 NAO satisfeitos)."
  exit 1
fi
echo "RLS conforme -- isolamento por residencia e por coluna comprovado (SC-003/SC-004)."
