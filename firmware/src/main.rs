//! Smoke test inicial: monta LittleFS, conecta Wi-Fi, sincroniza relogio via SNTP.
//!
//! A partir daqui voce constroi o dominio do novo projeto. Veja:
//!   - persistencia::littlefs::base()  -> "/lfs", grave arquivos do dominio aqui
//!   - comunicacao::supabase.rs.template -> referencia de cliente HTTPS Supabase

use anyhow::Result;
use chrono::Local;
use esp_idf_svc::hal::peripherals::Peripherals;
use std::thread;
use std::time::Duration;

mod comunicacao;
mod config_nuvem;
mod persistencia;

fn main() -> Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("=== y-firmware: smoke test de infra ===");

    // 1. LittleFS
    persistencia::init()?;
    let (total, used) = persistencia::littlefs::espaco_livre()?;
    log::info!("LittleFS pronto: {used} / {total} bytes em uso");

    // 2. Wi-Fi (STA + fallback AP de provisionamento)
    let peripherals = Peripherals::take()?;
    let _wifi = comunicacao::wifi::conectar(peripherals.modem)?;
    log::info!("Wi-Fi: conectado");

    // 3. SNTP -- sincroniza relogio para que `chrono::Local::now()` seja real
    comunicacao::sntp::sincronizar()?;
    log::info!("SNTP: relogio sincronizado -> {}", Local::now());

    log::info!("Infra pronta. Implemente o dominio do novo projeto a partir daqui.");

    loop {
        thread::sleep(Duration::from_secs(30));
        log::info!("heartbeat -- {}", Local::now());
    }
}
