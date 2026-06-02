# criar-claim.ps1 — cunha um claim-code de pareamento (provisioning_claims).
#
# O claim-code so pode ser criado com privilegio (service_role / dono). Este script roda
# na SUA maquina, NUNCA no ESP32 (Principio VIII). Ele:
#   1. gera (ou usa) um codigo em texto,
#   2. calcula sha256 hex (mesmo hash da Edge Function `provisionar`),
#   3. insere a linha em `provisioning_claims` via PostgREST com a service_role key,
#   4. imprime o codigo em texto p/ voce digitar no portal AP da tomada (ou no .env).
#
# Pre-requisitos (variaveis de ambiente na sessao):
#   $env:SUPABASE_URL = "https://<proj>.supabase.co"
#   $env:SUPABASE_SERVICE_ROLE_KEY = "<service_role secret>"   # Project Settings > API
#
# Uso:
#   ./scripts/criar-claim.ps1 -ResidenciaId <uuid>                 # codigo aleatorio, 2h
#   ./scripts/criar-claim.ps1 -ResidenciaId <uuid> -Codigo MEU-123 -Horas 6
#   ./scripts/criar-claim.ps1 -ListarResidencias                   # lista ids/nomes

[CmdletBinding()]
param(
    [string]$ResidenciaId,
    [string]$Codigo,
    [int]$Horas = 2,
    [switch]$ListarResidencias
)

$ErrorActionPreference = "Stop"

$Url = $env:SUPABASE_URL
if ([string]::IsNullOrWhiteSpace($Url)) { $Url = $env:PROJECT_URL }
$Key = $env:SUPABASE_SERVICE_ROLE_KEY
if ([string]::IsNullOrWhiteSpace($Key)) { $Key = $env:SERVICE_ROLE_KEY }

if ([string]::IsNullOrWhiteSpace($Url) -or [string]::IsNullOrWhiteSpace($Key)) {
    Write-Error "Defina `$env:SUPABASE_URL e `$env:SUPABASE_SERVICE_ROLE_KEY antes de rodar."
    exit 2
}
$Url = $Url.TrimEnd("/")

$headers = @{
    apikey        = $Key
    Authorization = "Bearer $Key"
    "Content-Type" = "application/json"
}

if ($ListarResidencias) {
    $r = Invoke-RestMethod -Method Get -Headers $headers `
        -Uri "$Url/rest/v1/residencias?select=id,nome,p_max"
    $r | Format-Table -AutoSize
    exit 0
}

if ([string]::IsNullOrWhiteSpace($ResidenciaId)) {
    Write-Error "Faltou -ResidenciaId <uuid>. Use -ListarResidencias para ver os ids."
    exit 2
}

# Codigo em texto (aleatorio se nao informado).
if ([string]::IsNullOrWhiteSpace($Codigo)) {
    $alfabeto = (48..57) + (65..90)   # 0-9 A-Z
    $sufixo = -join ($alfabeto | Get-Random -Count 6 | ForEach-Object { [char]$_ })
    $Codigo = "TOMADA-$sufixo"
}

# sha256 hex minusculo (igual ao sha256Hex da Edge Function).
$sha = [System.Security.Cryptography.SHA256]::Create()
$bytes = [System.Text.Encoding]::UTF8.GetBytes($Codigo)
$hash = (($sha.ComputeHash($bytes)) | ForEach-Object { $_.ToString("x2") }) -join ""

$expira = (Get-Date).ToUniversalTime().AddHours($Horas).ToString("yyyy-MM-ddTHH:mm:ssZ")

$body = @{
    residencia_id = $ResidenciaId
    code_hash     = $hash
    expira_em     = $expira
} | ConvertTo-Json -Compress

$postHeaders = $headers.Clone()
$postHeaders["Prefer"] = "return=minimal"

Invoke-RestMethod -Method Post -Headers $postHeaders `
    -Uri "$Url/rest/v1/provisioning_claims" -Body $body | Out-Null

Write-Host ""
Write-Host "Claim-code criado (uso unico, expira em $Horas h):" -ForegroundColor Green
Write-Host "  CODIGO     : $Codigo" -ForegroundColor Yellow
Write-Host "  residencia : $ResidenciaId"
Write-Host "  expira_em  : $expira (UTC)"
Write-Host ""
Write-Host "Digite esse CODIGO no portal AP da tomada (ou em firmware/.env: CLAIM_CODE=$Codigo)."
