//! Sincronizacao de relogio via SNTP.
//!
//! Sem isso, `SystemTime::now()` retorna apenas o uptime (segundos desde boot)
//! a partir de unix epoch 0. Com SNTP, o RTC interno do ESP32 e ajustado para
//! o tempo real e SystemTime/chrono passam a devolver timestamps validos.
//!
//! O EspSntp fica vivo em background fazendo re-sincronizacoes periodicas.
//! O HANDLE retornado precisa ser mantido vivo durante toda a execucao.

use anyhow::{anyhow, Result};
use esp_idf_svc::sntp::{EspSntp, SyncStatus};
use std::time::{Duration, Instant};

pub struct SntpHandle {
    _sntp: EspSntp<'static>,
}

/// Liga o servico SNTP e BLOQUEIA ate a primeira sincronizacao (max 30s).
/// Pre-condicao: Wi-Fi conectado.
pub fn sincronizar() -> Result<SntpHandle> {
    log::info!("[SNTP] iniciando sincronizacao...");
    let sntp = EspSntp::new_default()?;

    // Bloqueia ate o primeiro sync ou timeout
    let inicio = Instant::now();
    let timeout = Duration::from_secs(30);
    while sntp.get_sync_status() != SyncStatus::Completed {
        if inicio.elapsed() > timeout {
            return Err(anyhow!("[SNTP] timeout aguardando primeira sincronizacao"));
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    let agora = chrono::Local::now();
    log::info!(
        "[SNTP] sincronizado em {:.1}s -- relogio: {}",
        inicio.elapsed().as_secs_f32(),
        agora.format("%Y-%m-%d %H:%M:%S %Z")
    );

    Ok(SntpHandle { _sntp: sntp })
}
