//! Relogio: uptime sempre, hora certa quando der (feature 005, D1 / FR-001).
//!
//! O v1 BLOQUEAVA o boot por ate 30 s e devolvia `Err` no timeout — e o `main` fazia
//! `?` nisso. Resultado: numa casa sem internet a tomada entrava em boot-loop eterno,
//! mesmo com Wi-Fi local perfeito. Era a contradicao mais direta com o offline-first
//! (Principio VI).
//!
//! Agora:
//!   - `iniciar()` liga o SNTP e volta na hora (best-effort, nunca falha o boot);
//!   - `uptime_s()` (monotonico, sempre disponivel) e a base de TODOS os deltas
//!     internos: TTL de peer, janela de pareamento, cadencias;
//!   - `agora_epoch()` devolve hora real depois do 1o sync e uptime antes dele;
//!   - `checar_sync()` roda no cloud tick e, na TRANSICAO para sincronizado, devolve o
//!     deslocamento para o `main` corrigir os carimbos pendentes (FR-001a).

use esp_idf_svc::sntp::{EspSntp, SyncStatus};
// AtomicI64 nao existe no Xtensa (sem CAS de 64 bits): o offset vive num Mutex.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Handle vivo do servico (re-sincroniza periodicamente sozinho).
pub struct SntpHandle {
    sntp: EspSntp<'static>,
}

/// Ja houve pelo menos uma sincronizacao com a rede.
static RELOGIO_OK: AtomicBool = AtomicBool::new(false);
/// `epoch_real - uptime` fixado no 1o sync (tomada_solver::relogio::offset_do_sync).
static OFFSET_BOOT: Mutex<i64> = Mutex::new(0);

/// Segundos desde o boot (monotonico; independe de rede e de hora certa).
pub fn uptime_s() -> i64 {
    // esp_timer_get_time() e microssegundos desde o boot.
    (unsafe { esp_idf_svc::sys::esp_timer_get_time() } / 1_000_000) as i64
}

/// Ha hora certa? Quem depende de hora do dia (agenda/historico) consulta isto
/// antes de aplicar a regra (FR-001b).
pub fn relogio_ok() -> bool {
    RELOGIO_OK.load(Ordering::Relaxed)
}

/// Deslocamento atual (0 enquanto nao houve sync).
pub fn offset() -> i64 {
    *OFFSET_BOOT.lock().unwrap()
}

/// Epoch em segundos: hora real apos o 1o sync, uptime antes dele.
///
/// Antes do sync os valores sao pequenos (segundos desde o boot) — nunca uma data
/// plausivel de 1970 mascarada de agora. Quem persiste medicao usa
/// `tomada_solver::relogio::SEM_HORA` como sentinela, nao este valor.
pub fn agora_epoch() -> i64 {
    let up = uptime_s();
    if relogio_ok() {
        tomada_solver::relogio::epoch_de(up, offset())
    } else {
        up
    }
}

/// Liga o SNTP. NUNCA falha o boot: sem rede, o servico so fica tentando.
pub fn iniciar() -> SntpHandle {
    log::info!("[SNTP] servico iniciado (nao-bloqueante)");
    match EspSntp::new_default() {
        Ok(sntp) => SntpHandle { sntp },
        Err(e) => {
            // Mesmo sem o servico, o no opera com uptime. Reiniciar o SNTP depois nao
            // vale a complexidade: sem rede no boot, o Wi-Fi tambem reconecta e o
            // proximo boot tenta de novo.
            log::warn!("[SNTP] indisponivel ({e:?}); operando so com uptime");
            SntpHandle {
                sntp: EspSntp::new_default().unwrap_or_else(|_| {
                    panic!("[SNTP] servico indisponivel duas vezes seguidas")
                }),
            }
        }
    }
}

/// Chamada periodicamente (cloud tick). Devolve `Some(offset)` UMA vez: no instante em
/// que a hora certa chega — e o gatilho para corrigir os carimbos pendentes.
pub fn checar_sync(h: &SntpHandle) -> Option<i64> {
    if relogio_ok() {
        return None;
    }
    if h.sntp.get_sync_status() != SyncStatus::Completed {
        return None;
    }
    // Agora `SystemTime` reflete a hora real.
    let epoch_real = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    if epoch_real <= 0 {
        return None;
    }
    let off = tomada_solver::relogio::offset_do_sync(epoch_real, uptime_s());
    *OFFSET_BOOT.lock().unwrap() = off;
    RELOGIO_OK.store(true, Ordering::Relaxed);
    log::info!(
        "[SNTP] hora certa obtida ({}) -- offset={off}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S %Z")
    );
    Some(off)
}
