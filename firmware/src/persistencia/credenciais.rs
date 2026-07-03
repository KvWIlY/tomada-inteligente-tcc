//! Credenciais por-dispositivo provisionadas em runtime (Principio VIII).
//!
//! Persistidas em LittleFS (`/lfs/cred.json`), NUNCA no `.bin` (data-model §2bis,
//! provisioning-api.md). Separa identidade (`mac` -> `tomada_id`/`residencia_id`) de
//! autenticacao (`device_secret` duravel + `jwt` curto renovavel). O `device_secret` so
//! existe aqui e como hash no servidor; o `jwt`/`jwt_exp` sao cache renovavel.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;

use crate::persistencia::littlefs;

/// Credenciais do dispositivo. `jwt`/`jwt_exp` sao cache de regime (renovaveis).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredenciaisLocais {
    /// MAC do efuso (identidade de hardware; = `tomadas.mac`). NAO e segredo.
    pub mac: String,
    /// `tomada_id` UUID (claim do JWT; identifica a linha em `tomadas`).
    pub tomada_id: String,
    /// `residencia_id` UUID (claim do JWT; agrupa o conjunto I).
    pub residencia_id: String,
    /// Segredo duravel (refresh). Trocado por JWT novo via `renovar`. So texto aqui.
    pub device_secret: String,
    /// Chave HMAC compartilhada da residencia (32 bytes hex) para autenticar o ESP-NOW
    /// (protocolo v3). Mesma para todos os devices da casa; vem do `provisionar`. Vazia em
    /// credenciais antigas (anteriores a este campo) => ESP-NOW fica desabilitado ate
    /// re-provisionar (fail-closed).
    #[serde(default)]
    pub residencia_key: String,
    /// JWT de regime (cache). Vazio ate a 1a obtencao/renovacao.
    #[serde(default)]
    pub jwt: String,
    /// Epoch de expiracao do JWT (`exp`). 0 = sem token valido em cache.
    #[serde(default)]
    pub jwt_exp: i64,
}

fn caminho() -> String {
    format!("{}/cred.json", littlefs::base())
}

/// Le as credenciais provisionadas. `Ok(None)` se o dispositivo ainda nao foi
/// provisionado (arquivo ausente) — o caller deve disparar o fluxo de provisionamento.
pub fn load() -> Result<Option<CredenciaisLocais>> {
    let path = caminho();
    match std::fs::read_to_string(&path) {
        Ok(s) => {
            let c: CredenciaisLocais = serde_json::from_str(&s)
                .with_context(|| format!("cred.json invalido em {path}"))?;
            Ok(Some(c))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("falha lendo {path}")),
    }
}

/// Persiste o conjunto completo de credenciais (apos `provisionar`).
pub fn save(c: &CredenciaisLocais) -> Result<()> {
    let path = caminho();
    let json = serde_json::to_string(c).context("serializando credenciais")?;
    let mut f = std::fs::File::create(&path).with_context(|| format!("criando {path}"))?;
    f.write_all(json.as_bytes())?;
    f.flush()?;
    // NUNCA logar device_secret/jwt (Principio VII/VIII).
    log::info!("[CRED] gravado {path} (tomada_id={})", c.tomada_id);
    Ok(())
}

/// Atualiza somente o cache de JWT (apos `renovar`/`provisionar`) e regrava.
/// Renovacao e infrequente (TTL 1..24h) — escrita pontual, nao desgasta a flash.
pub fn salvar_jwt(jwt: &str, jwt_exp: i64) -> Result<()> {
    let mut c = load()?.context("salvar_jwt sem credencial provisionada")?;
    c.jwt = jwt.to_string();
    c.jwt_exp = jwt_exp;
    save(&c)
}

// ============================================================
// Claim-code pendente (coletado no portal AP; sobrevive ao restart do Wi-Fi).
// O device NUNCA cunha o claim — so o transporta do portal ate `provisionar`.
// ============================================================

fn caminho_claim() -> String {
    format!("{}/claim.txt", littlefs::base())
}

/// Persiste o claim-code digitado no portal (consumido no proximo boot). Vazio => no-op.
pub fn salvar_claim_pendente(claim: &str) -> Result<()> {
    let claim = claim.trim();
    if claim.is_empty() {
        return Ok(());
    }
    let path = caminho_claim();
    let mut f = std::fs::File::create(&path).with_context(|| format!("criando {path}"))?;
    f.write_all(claim.as_bytes())?;
    f.flush()?;
    log::info!("[CRED] claim-code do portal salvo p/ provisionamento no proximo boot");
    Ok(())
}

/// Le o claim-code pendente (do portal). `None` se ausente/vazio.
pub fn ler_claim_pendente() -> Option<String> {
    let s = std::fs::read_to_string(caminho_claim()).ok()?;
    let s = s.trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Remove o claim-code pendente apos consumo (uso unico).
pub fn limpar_claim_pendente() {
    let _ = std::fs::remove_file(caminho_claim());
}

/// Apaga TODA a identidade provisionada (cred.json + claim pendente). Usado no reset de
/// build novo (dev) para forcar um reprovisionamento limpo. Best-effort (ignora ausencia).
pub fn limpar_tudo() {
    let _ = std::fs::remove_file(caminho());
    let _ = std::fs::remove_file(caminho_claim());
}
