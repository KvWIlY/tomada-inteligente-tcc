//! Provisionamento e autenticacao por-dispositivo (Principio VIII).
//!
//! Implementa o cliente dos endpoints da Edge Function (`provisioning-api.md`):
//!   - `provisionar(mac, claim_code)` -> registra a tomada e devolve segredo + 1o JWT.
//!   - `renovar(tomada_id, device_secret)` -> troca o segredo por um JWT novo.
//! E o provedor de JWT de regime para o cliente Supabase:
//!   - `obter_jwt_valido()` renova proativamente por margem antes de `jwt_exp`
//!     (logica pura `tomada_solver::sync::deve_renovar`); `renovar_agora()` forca (em 401).
//!
//! O segredo de assinatura do JWT vive SO no servidor; aqui so consumimos. As chamadas a
//! `/functions/v1/*` levam a publishable key (publica) — o gateway exige `apikey`.

use anyhow::{anyhow, Context, Result};
use embedded_svc::http::client::Client;
use embedded_svc::http::Method;
use embedded_svc::io::Write as _;
use esp_idf_svc::http::client::{Configuration as HttpConfig, EspHttpConnection};
use serde::Deserialize;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config_nuvem::{SUPABASE_PUBLISHABLE_KEY, SUPABASE_URL};
use crate::persistencia::credenciais::{self, CredenciaisLocais};
use tomada_solver::sync::deve_renovar;

/// Margem de renovacao proativa: renova quando faltam <= 5 min para `exp`.
const MARGEM_RENOVACAO_S: i64 = 300;

/// Epoch atual (s). Requer SNTP ja sincronizado.
pub fn agora_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// MAC base do efuso como bytes (identidade de hardware; tambem usado como chave de
/// ordenacao canonica na coordenacao ESP-NOW — ver espnow-protocol.md).
pub fn obter_mac_bytes() -> Result<[u8; 6]> {
    let mut mac = [0u8; 6];
    let err = unsafe { esp_idf_svc::sys::esp_efuse_mac_get_default(mac.as_mut_ptr()) };
    if err != esp_idf_svc::sys::ESP_OK as i32 {
        return Err(anyhow!("esp_efuse_mac_get_default falhou (cod {err})"));
    }
    Ok(mac)
}

/// MAC base do efuso, formatado `AA:BB:CC:DD:EE:FF` (identidade de hardware).
pub fn obter_mac() -> Result<String> {
    let mac = obter_mac_bytes()?;
    Ok(format!(
        "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
    ))
}

/// Erro de provisionamento classificado para o loop de boot decidir o que fazer:
/// claim ruim (reabrir o portal — retry com o mesmo claim e inutil) vs transitorio
/// (rede/5xx/parse — pode resolver sozinho, vale tentar de novo).
pub enum ErroProv {
    /// HTTP 401: claim invalido/expirado/usado.
    ClaimInvalido,
    /// Falha que pode ser transitoria.
    Transitorio(anyhow::Error),
}

#[derive(Deserialize)]
struct RespProvisionar {
    tomada_id: String,
    residencia_id: String,
    device_secret: String,
    #[serde(default)]
    residencia_key: String,
    jwt: String,
    jwt_exp: i64,
}

#[derive(Deserialize)]
struct RespRenovar {
    jwt: String,
    jwt_exp: i64,
}

/// 1o boot: chama `provisionar`, grava credenciais em LittleFS e retorna-as.
/// `claim_code` vem do portal AP (dashboard do dono).
pub fn provisionar(mac: &str, claim_code: &str) -> Result<CredenciaisLocais, ErroProv> {
    let url = format!("{}/functions/v1/provisionar", SUPABASE_URL);
    // Corpo so com strings (sem f32) -> serde_json seguro.
    let body = format!(
        r#"{{"mac":"{}","claim_code":"{}"}}"#,
        escape_json(mac),
        escape_json(claim_code)
    );
    let (status, resp) = post_funcao(&url, &body).map_err(ErroProv::Transitorio)?;
    // 401 = claim invalido/expirado/usado: retry com o mesmo claim e inutil -> reabrir portal.
    if status == 401 {
        return Err(ErroProv::ClaimInvalido);
    }
    if !(200..=299).contains(&status) {
        return Err(ErroProv::Transitorio(anyhow!(
            "[PROV] provisionar HTTP {status}: {resp}"
        )));
    }
    let p: RespProvisionar = serde_json::from_str(&resp)
        .context("parse resposta de provisionar")
        .map_err(ErroProv::Transitorio)?;
    let creds = CredenciaisLocais {
        mac: mac.to_string(),
        tomada_id: p.tomada_id,
        residencia_id: p.residencia_id,
        device_secret: p.device_secret,
        residencia_key: p.residencia_key,
        jwt: p.jwt,
        jwt_exp: p.jwt_exp,
    };
    credenciais::save(&creds).map_err(ErroProv::Transitorio)?;
    log::info!("[PROV] provisionado (tomada_id={})", creds.tomada_id);
    Ok(creds)
}

/// Troca o `device_secret` por um JWT novo. Retorna `(jwt, jwt_exp)`.
/// A renovacao e servida como subpath da funcao `provisionar` (roteamento interno
/// `action == "renovar"`), nao como funcao separada — por isso `provisionar/renovar`.
pub fn renovar(tomada_id: &str, device_secret: &str) -> Result<(String, i64)> {
    let url = format!("{}/functions/v1/provisionar/renovar", SUPABASE_URL);
    let body = format!(
        r#"{{"tomada_id":"{}","device_secret":"{}"}}"#,
        escape_json(tomada_id),
        escape_json(device_secret)
    );
    let (status, resp) = post_funcao(&url, &body)?;
    if status == 401 {
        return Err(anyhow!("[PROV] renovar 401 — credencial revogada/invalida"));
    }
    if !(200..=299).contains(&status) {
        return Err(anyhow!("[PROV] renovar HTTP {status}: {resp}"));
    }
    let r: RespRenovar = serde_json::from_str(&resp).context("parse resposta de renovar")?;
    Ok((r.jwt, r.jwt_exp))
}

/// JWT valido para uso no PostgREST: renova proativamente se perto de expirar.
pub fn obter_jwt_valido() -> Result<String> {
    let creds = credenciais::load()?.context("obter_jwt_valido: sem credencial")?;
    if creds.jwt.is_empty() || deve_renovar(creds.jwt_exp, agora_epoch(), MARGEM_RENOVACAO_S) {
        return renovar_agora();
    }
    Ok(creds.jwt)
}

/// Forca a renovacao (usado pelo cliente Supabase apos `401/403`). Persiste o novo JWT.
pub fn renovar_agora() -> Result<String> {
    let creds = credenciais::load()?.context("renovar_agora: sem credencial")?;
    let (jwt, exp) = renovar(&creds.tomada_id, &creds.device_secret)?;
    credenciais::salvar_jwt(&jwt, exp)?;
    log::info!("[PROV] JWT renovado (exp={exp})");
    Ok(jwt)
}

// ============================================================
// HTTP helper para a Edge Function (apikey publishable; sem JWT de device ainda).
// ============================================================

fn post_funcao(url: &str, body: &str) -> Result<(u16, String)> {
    let content_length = body.len().to_string();
    let bearer = format!("Bearer {SUPABASE_PUBLISHABLE_KEY}");
    let headers: [(&str, &str); 4] = [
        ("apikey", SUPABASE_PUBLISHABLE_KEY),
        ("Authorization", bearer.as_str()),
        ("Content-Type", "application/json"),
        ("Content-Length", content_length.as_str()),
    ];
    let config = HttpConfig {
        use_global_ca_store: true,
        crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
        timeout: Some(std::time::Duration::from_secs(15)),
        // Folga p/ headers (resposta com muitos headers do gateway + Bearer longo no request).
        buffer_size: Some(4096),
        buffer_size_tx: Some(2048),
        ..Default::default()
    };
    let mut client = Client::wrap(EspHttpConnection::new(&config)?);
    let mut req = client.request(Method::Post, url, &headers)?;
    req.write_all(body.as_bytes())?;
    req.flush()?;
    let mut resp = req.submit()?;
    let status = resp.status();

    let mut buf_resp = Vec::new();
    let mut buf = [0u8; 512];
    loop {
        match resp.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => buf_resp.extend_from_slice(&buf[..n]),
            Err(e) => return Err(anyhow!("[PROV] leitura da resposta falhou: {e:?}")),
        }
    }
    let texto = String::from_utf8_lossy(&buf_resp).to_string();
    Ok((status, texto))
}

/// Escapa `\` e `"` para uso dentro de aspas em JSON.
fn escape_json(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
