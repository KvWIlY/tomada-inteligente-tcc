<#
.SYNOPSIS
  Verificacao de isolamento RLS ponta-a-ponta (feature 002-refinamento-supabase).

.DESCRIPTION
  Exercita os 15 casos de specs/002-refinamento-supabase/contracts/rls-verification.md
  pelo caminho REAL do cliente (HTTP + Authorization: Bearer <jwt device>), cobrindo
  SC-003/SC-004 e FR-006/007/008/009. Imprime PASS/FAIL por caso e termina com exit 1
  em qualquer divergencia.

  "negado" = HTTP 401/403  OU  2xx afetando 0 linhas (array JSON vazio). O criterio e:
  nenhum dado de outro inquilino lido/alterado e nenhuma coluna reservada escrita.

.NOTES
  Nota: o gateway do Supabase exige o header `apikey` (publishable/anon, PUBLICA) em toda
  requisicao; a auth/isolamento real e o `Authorization: Bearer <jwt device>` + RLS.

  Defina antes de rodar (NUNCA use segredos de producao; use um projeto de teste):
    $env:SUPABASE_URL = "https://<proj-teste>.supabase.co"
    $env:SUPABASE_APIKEY = "<publishable key OU anon key>"
    $env:JWT_A = "..."; $env:JWT_B = "..."
    $env:A_T1 = "<uuid tomada A.t1>"; $env:RESID_A = "<uuid residencia A>"
    $env:B_T1 = "<uuid tomada B.t1>"; $env:RESID_B = "<uuid residencia B>"
  Uso:  powershell -File scripts/verify-rls.ps1
#>

$ErrorActionPreference = 'Stop'

# ---- pre-condicoes ----
$faltando = $false
foreach ($v in 'SUPABASE_URL','SUPABASE_APIKEY','JWT_A','JWT_B','A_T1','RESID_A','B_T1','RESID_B') {
  if ([string]::IsNullOrEmpty([Environment]::GetEnvironmentVariable($v))) {
    Write-Error "Variavel de ambiente '$v' nao definida."
    $faltando = $true
  }
}
if ($faltando) { Write-Host "Defina as variaveis acima e rode de novo." ; exit 2 }

$BASE = ($env:SUPABASE_URL.TrimEnd('/')) + "/rest/v1"
$TS   = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
$script:PASS = 0
$script:FAIL = 0

# Invoca uma requisicao e devolve [pscustomobject]@{ Status; Body }.
function Invoke-Req {
  param([string]$Method,[string]$Path,[string]$Jwt,[string]$Body,[string]$Prefer)
  # apikey (publishable, publica) em toda requisicao -- exigencia do gateway.
  $headers = @{ 'Content-Type' = 'application/json'; 'apikey' = $env:SUPABASE_APIKEY }
  if ($Jwt -ne 'ANON') { $headers['Authorization'] = "Bearer $Jwt" }
  if ($Prefer)         { $headers['Prefer'] = $Prefer }
  $p = @{ Uri = "$BASE$Path"; Method = $Method; Headers = $headers; SkipHttpErrorCheck = $true }
  if ($Body) { $p['Body'] = $Body }
  try {
    $resp = Invoke-WebRequest @p
    [pscustomobject]@{ Status = [int]$resp.StatusCode; Body = [string]$resp.Content }
  } catch {
    # PS 5.1 sem SkipHttpErrorCheck: extrai status do WebException
    $code = -1
    if ($_.Exception.Response) { $code = [int]$_.Exception.Response.StatusCode }
    [pscustomobject]@{ Status = $code; Body = '' }
  }
}

function Is-2xx([int]$s)    { $s -ge 200 -and $s -le 299 }
function Is-Denied([int]$s) { $s -eq 401 -or $s -eq 403 }
function Is-EmptyArr([string]$b) {
  $t = ($b -replace '\s','')
  [string]::IsNullOrEmpty($t) -or $t -eq '[]'
}
function Pass([int]$n,[string]$m) { $script:PASS++; Write-Host ("  PASS  #{0,-2} {1}" -f $n,$m) }
function Fail([int]$n,[string]$m,[int]$s,[string]$b) {
  $script:FAIL++
  Write-Host ("  FAIL  #{0,-2} {1}" -f $n,$m) -ForegroundColor Red
  $snip = if ($b.Length -gt 200) { $b.Substring(0,200) } else { $b }
  Write-Host ("        -> status=$s body=$snip")
}

# ---- Operacoes LEGITIMAS (permitidas): 1, 2, 5, 9, 11 ----

$r = Invoke-Req GET "/tomadas?id=eq.$($env:A_T1)&select=id" $env:JWT_A
if ((Is-2xx $r.Status) -and $r.Body -match [regex]::Escape($env:A_T1)) { Pass 1 "A.t1 GET propria tomada (1 linha)" } else { Fail 1 "A.t1 GET propria tomada (esperado 1 linha)" $r.Status $r.Body }

$r = Invoke-Req GET "/tomadas?residencia_id=eq.$($env:RESID_A)&select=id" $env:JWT_A
if ((Is-2xx $r.Status) -and -not (Is-EmptyArr $r.Body)) { Pass 2 "A.t1 GET irmas (mesma residencia, >=1 linha)" } else { Fail 2 "A.t1 GET irmas (esperado >=1 linha)" $r.Status $r.Body }

$r = Invoke-Req PATCH "/tomadas?id=eq.$($env:A_T1)" $env:JWT_A '{"estado":"ON"}' 'return=representation'
if ((Is-2xx $r.Status) -and -not (Is-EmptyArr $r.Body)) { Pass 5 "A.t1 PATCH estado (coluna permitida)" } else { Fail 5 "A.t1 PATCH estado (esperado 2xx, 1 linha)" $r.Status $r.Body }

# on_conflict=tomada_id,timestamp faz o ON CONFLICT casar na chave NATURAL (nao na PK), habilitando idempotencia (FR-013).
$medA = '{"tomada_id":"' + $env:A_T1 + '","timestamp":' + $TS + ',"p_medida":1.00,"estado":"ON","rele_aberto":false}'
$r = Invoke-Req POST "/medicoes?on_conflict=tomada_id,timestamp" $env:JWT_A $medA 'return=minimal,resolution=ignore-duplicates'
if (Is-2xx $r.Status) { Pass 9 "A.t1 POST medicao propria" } else { Fail 9 "A.t1 POST medicao propria (esperado 2xx)" $r.Status $r.Body }

$r = Invoke-Req POST "/medicoes?on_conflict=tomada_id,timestamp" $env:JWT_A $medA 'return=minimal,resolution=ignore-duplicates'
if (Is-2xx $r.Status) { Pass 11 "A.t1 reenvio idempotente da medicao" } else { Fail 11 "A.t1 reenvio idempotente (esperado 2xx)" $r.Status $r.Body }

# ---- Operacoes NEGADAS/VAZIAS: 3, 4, 6, 7, 8, 10, 12, 13, 14, 15 ----

$r = Invoke-Req GET "/tomadas?residencia_id=eq.$($env:RESID_B)&select=id" $env:JWT_A
if (((Is-2xx $r.Status) -and (Is-EmptyArr $r.Body)) -or (Is-Denied $r.Status)) { Pass 3 "A.t1 GET residencia B negado/vazio" } else { Fail 3 "A.t1 GET residencia B (esperado vazio)" $r.Status $r.Body }

$r = Invoke-Req GET "/tomadas?id=eq.$($env:B_T1)&select=id" $env:JWT_A
if (((Is-2xx $r.Status) -and (Is-EmptyArr $r.Body)) -or (Is-Denied $r.Status)) { Pass 4 "A.t1 GET tomada de B negado/vazio" } else { Fail 4 "A.t1 GET tomada de B (esperado vazio)" $r.Status $r.Body }

$r = Invoke-Req PATCH "/tomadas?id=eq.$($env:A_T1)" $env:JWT_A '{"rele_aberto":true}' 'return=representation'
if ((Is-Denied $r.Status) -or ((Is-2xx $r.Status) -and (Is-EmptyArr $r.Body))) { Pass 6 "A.t1 PATCH rele_aberto negado" } else { Fail 6 "A.t1 PATCH rele_aberto (esperado negado)" $r.Status $r.Body }

$r = Invoke-Req PATCH "/residencias?id=eq.$($env:RESID_A)" $env:JWT_A '{"p_max":99}' 'return=representation'
if ((Is-Denied $r.Status) -or ((Is-2xx $r.Status) -and (Is-EmptyArr $r.Body))) { Pass 7 "A.t1 PATCH residencias.p_max negado" } else { Fail 7 "A.t1 PATCH p_max (esperado negado)" $r.Status $r.Body }

$r = Invoke-Req PATCH "/tomadas?id=eq.$($env:B_T1)" $env:JWT_A '{"estado":"OFF"}' 'return=representation'
if ((Is-Denied $r.Status) -or ((Is-2xx $r.Status) -and (Is-EmptyArr $r.Body))) { Pass 8 "A.t1 PATCH linha de B negado/0 linhas" } else { Fail 8 "A.t1 PATCH linha de B (esperado negado/vazio)" $r.Status $r.Body }

$medB = '{"tomada_id":"' + $env:B_T1 + '","timestamp":' + $TS + ',"p_medida":1.00,"estado":"ON","rele_aberto":false}'
$r = Invoke-Req POST "/medicoes" $env:JWT_A $medB 'return=representation'
if ((Is-Denied $r.Status) -or ((Is-2xx $r.Status) -and (Is-EmptyArr $r.Body))) { Pass 10 "A.t1 POST medicao de B negado" } else { Fail 10 "A.t1 POST medicao de B (esperado negado)" $r.Status $r.Body }

$r = Invoke-Req GET "/medicoes?tomada_id=eq.$($env:B_T1)&select=tomada_id" $env:JWT_A
if (((Is-2xx $r.Status) -and (Is-EmptyArr $r.Body)) -or (Is-Denied $r.Status)) { Pass 12 "A.t1 GET medicoes de B negado/vazio" } else { Fail 12 "A.t1 GET medicoes de B (esperado vazio)" $r.Status $r.Body }

$r = Invoke-Req GET "/device_credentials?select=tomada_id" $env:JWT_A
if ((Is-Denied $r.Status) -or ((Is-2xx $r.Status) -and (Is-EmptyArr $r.Body))) { Pass 13 "A.t1 GET device_credentials negado/vazio" } else { Fail 13 "A.t1 GET device_credentials (esperado negado)" $r.Status $r.Body }

$r = Invoke-Req GET "/provisioning_claims?select=id" $env:JWT_A
if ((Is-Denied $r.Status) -or ((Is-2xx $r.Status) -and (Is-EmptyArr $r.Body))) { Pass 14 "A.t1 GET provisioning_claims negado/vazio" } else { Fail 14 "A.t1 GET provisioning_claims (esperado negado)" $r.Status $r.Body }

$r = Invoke-Req GET "/tomadas?select=id" "ANON"
if ((Is-Denied $r.Status) -or ((Is-2xx $r.Status) -and (Is-EmptyArr $r.Body))) { Pass 15 "anon GET tomadas negado/vazio" } else { Fail 15 "anon GET tomadas (esperado negado)" $r.Status $r.Body }

# ----
Write-Host ""
Write-Host ("Resultado: {0} PASS / {1} FAIL (de 15 casos)" -f $script:PASS, $script:FAIL)
if ($script:FAIL -ne 0) {
  Write-Host "RLS NAO conforme -- ver casos FAIL acima (SC-003/SC-004 NAO satisfeitos)."
  exit 1
}
Write-Host "RLS conforme -- isolamento por residencia e por coluna comprovado (SC-003/SC-004)."
