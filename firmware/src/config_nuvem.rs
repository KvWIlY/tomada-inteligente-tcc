//! Configuracoes de BOOTSTRAP lidas do .env em TEMPO DE COMPILACAO.
//!
//! O `dotenv!()` da crate `dotenvy_macro` e uma proc-macro: durante o `cargo build`
//! ela le o arquivo `.env` na raiz do crate e substitui a chamada por uma `&'static str`
//! literal no binario. O `.bin` final e self-contained -- o `.env` nao precisa estar
//! no chip nem em runtime.
//!
//! SEGURANCA (Principio VIII / feature 004, FR-002): este arquivo expoe SOMENTE a
//! superficie minima de bootstrap -- endereco do backend na LAN + credenciais de Wi-Fi.
//! NAO ha NENHUM segredo aqui:
//!   - nao ha chave de API (o backend proprio nao tem gateway exigindo `apikey`);
//!   - nao ha identificadores por-dispositivo (`TOMADA_ID`/`RESIDENCIA_ID`);
//!   - nao ha claim-code em compile-time -- ele e coletado no Portal AP
//!     (`comunicacao::wifi_provisioning`) e vive so em LittleFS ate ser consumido.
//! Identidade e auth vem do provisionamento em runtime (`persistencia::credenciais`:
//! `tomada_id`/`residencia_id`/`device_secret`/`jwt`), nunca do binario. O `.bin` e
//! generico (SC-001/SC-002/SC-006).
//!
//! Esperado em `.env` na raiz deste crate (y/firmware/.env):
//!   WIFI_SSID=...
//!   WIFI_PASSWORD=...
//!   BACKEND_URL=http://192.168.0.10:8080

use dotenvy_macro::dotenv;

// --- Wi-Fi (bootstrap) ---
pub const WIFI_SSID: &str = dotenv!("WIFI_SSID");
pub const WIFI_PASSWORD: &str = dotenv!("WIFI_PASSWORD");

// --- Backend proprio (LAN) ---
/// Base URL da API vinda do build, SEM barra final. Ex.: `http://192.168.0.10:8080`.
/// A PARTIR DA 005 isto e apenas o valor DEFAULT do campo "servidor" do Portal AP: o
/// endereco efetivo vive na NVS e e lido por `backend_url()` (FR-014). Assim, trocar o
/// IP do servidor nao exige regravar o firmware.
/// LAN-only nesta iteracao (clarify Q2); o cliente HTTP mantem o cert bundle
/// habilitado para expor via HTTPS no futuro sem reescrita.
pub const BACKEND_URL_BUILD: &str = dotenv!("BACKEND_URL");

/// Namespace/chave NVS do endereco configurado no portal.
const NVS_NAMESPACE: &str = "wifi_creds";
const NVS_KEY_SERVIDOR: &str = "servidor_url";

/// Endereco efetivo do backend: NVS (portal) -> `.env` do build (fallback).
///
/// Lido a cada chamada (nao cacheado): o custo e irrelevante perto do HTTP que vem a
/// seguir, e evita estado global obsoleto depois de uma reconfiguracao pelo portal.
pub fn backend_url() -> String {
    if let Some(u) = ler_servidor_nvs() {
        if !u.is_empty() {
            return u;
        }
    }
    BACKEND_URL_BUILD.to_string()
}

fn ler_servidor_nvs() -> Option<String> {
    use esp_idf_svc::nvs::{EspDefaultNvsPartition, EspNvs};
    let part = EspDefaultNvsPartition::take().ok()?;
    let nvs = EspNvs::new(part, NVS_NAMESPACE, true).ok()?;
    let mut buf = [0u8; 160];
    nvs.get_str(NVS_KEY_SERVIDOR, &mut buf)
        .ok()
        .flatten()
        .map(|s| s.trim().trim_end_matches('/').to_string())
}

/// Persiste o endereco escolhido no portal (FR-014).
pub fn salvar_servidor(url: &str) -> anyhow::Result<()> {
    use esp_idf_svc::nvs::{EspDefaultNvsPartition, EspNvs};
    let part = EspDefaultNvsPartition::take()?;
    let nvs = EspNvs::new(part, NVS_NAMESPACE, true)?;
    nvs.set_str(NVS_KEY_SERVIDOR, url.trim().trim_end_matches('/'))?;
    log::info!("[CFG] endereco do backend salvo: {url}");
    Ok(())
}
