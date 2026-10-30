//! Calibracao dos sensores (ensaio de bancada, etapa 3): fecha o rele e imprime a cada 2 s os
//! mV brutos no ADC e V, I, P, FP com os fatores atuais de `zmpt101b.rs` e `sct013.rs`.
//! Usa o mesmo `sensores::medir` do firmware. Sem Wi-Fi, sem decisao.
//!
//! cd firmware && cargo build --example calibrar
//! espflash flash --port COM3 --partition-table partitions.csv ../target/xtensa-esp32-espidf/debug/examples/calibrar
//!
//! Fator novo = fator atual * (valor do multimetro / valor impresso).

#[allow(dead_code)]
#[path = "../src/sensores/mod.rs"]
mod sensores;

use std::time::Duration;

use esp_idf_svc::hal::adc::attenuation::DB_12;
use esp_idf_svc::hal::adc::oneshot::config::AdcChannelConfig;
use esp_idf_svc::hal::adc::oneshot::{AdcChannelDriver, AdcDriver};
use esp_idf_svc::hal::gpio::PinDriver;
use esp_idf_svc::hal::peripherals::Peripherals;

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let p = Peripherals::take()?;
    let adc1 = AdcDriver::new(p.adc1)?;
    let cfg = AdcChannelConfig {
        attenuation: DB_12,
        ..Default::default()
    };
    let mut ch_v = AdcChannelDriver::new(&adc1, p.pins.gpio32, &cfg)?;
    let mut ch_i = AdcChannelDriver::new(&adc1, p.pins.gpio35, &cfg)?;
    let mut rele = PinDriver::output(p.pins.gpio26)?;

    println!(
        "[CAL] fatores: V={} I={}",
        sensores::zmpt101b::FATOR_TENSAO,
        sensores::sct013::FATOR_CORRENTE
    );
    // O modulo de rele da bancada e ativo em BAIXO (conferido em 2026-10-27): LOW fecha.
    rele.set_low()?;
    loop {
        let (l, d) = sensores::medir(
            || ch_v.read().unwrap_or(0) as f32,
            || ch_i.read().unwrap_or(0) as f32,
        );
        println!(
            "[CAL] V_dc={:.0}mV V_ac={:.1}mV I_dc={:.0}mV I_ac={:.2}mV | V={:.1} I={:.3} P={:.1} S={:.1} FP={:.2}",
            d.v_dc_mv, d.v_ac_mv, d.i_dc_mv, d.i_ac_mv, l.vrms, l.irms, l.p, l.s, l.fp
        );
        std::thread::sleep(Duration::from_secs(2));
    }
}
