//! Botao de provisionamento (BOOT/GPIO0): segurar ~3s reabre o portal AP a qualquer
//! momento, mesmo com Wi-Fi ja salvo -- permite reconfigurar a rede sem reflashar nem
//! apagar o NVS (resolve o Caso 2: rede caiu com a tomada ligada).
//!
//! Como funciona: uma task le o GPIO0 a cada 100 ms; ao detectar 3 s continuos de
//! pressao, pede provisionamento (grava flag no NVS) e reinicia. No proximo boot,
//! `wifi::conectar` ve a flag, pula o STA e sobe direto o AP `Tomada-XXXX`.
//!
//! GPIO0 e o botao BOOT da DevKit (sem fiacao extra). E pino de strapping: se estiver
//! pressionado NO reset, o chip entra em modo download. Por isso a task espera 2 s antes
//! de reiniciar e avisa para SOLTAR o botao -- a leitura aqui e em runtime, nao no reset.

use anyhow::Result;
use esp_idf_svc::hal::gpio::{Gpio0, Input, PinDriver, Pull};
use std::time::Duration;

/// Tempo continuo de pressao para disparar o provisionamento.
const SEGURAR_MS: u64 = 3000;
/// Intervalo de amostragem do botao.
const POLL_MS: u64 = 100;

/// Sobe a task de monitoramento do botao. Best-effort: se o pino falhar, o caller so loga.
pub fn iniciar(pino: Gpio0<'static>) -> Result<()> {
    // Pull-up: solto = HIGH; pressionado (botao BOOT puxa p/ GND) = LOW.
    let botao = PinDriver::input(pino, Pull::Up)?;

    std::thread::Builder::new()
        .name("botao-prov".into())
        .stack_size(4096)
        .spawn(move || loop_botao(botao))?;
    log::info!("[BOTAO] monitor de provisionamento ativo (segure BOOT/GPIO0 por 3s)");
    Ok(())
}

fn loop_botao(botao: PinDriver<'static, Input>) {
    let mut segurado_ms: u64 = 0;
    loop {
        std::thread::sleep(Duration::from_millis(POLL_MS));

        if botao.is_low() {
            segurado_ms += POLL_MS;
            if segurado_ms == SEGURAR_MS {
                // Dispara so uma vez ao cruzar o limiar (== em vez de >=).
                log::warn!("[BOTAO] 3s de pressao detectados -> forcando provisionamento");
                crate::comunicacao::wifi::solicitar_provisionamento_e_reiniciar();
            }
        } else {
            if segurado_ms > 0 {
                log::info!("[BOTAO] solto antes dos 3s (segurou {segurado_ms}ms)");
            }
            segurado_ms = 0;
        }
    }
}
