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

/// Piso de ruido do SCT sem carga (mV-RMS no ADC): 6,3-7,4 mV medidos em 2026-11-01, com o rele
/// aberto e fechado (ja foi 3,8-4,9 mV em 2026-10-31, depende da montagem).
const RUIDO_I_MV: f32 = 7.0;

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
        // Abaixo de ~20 mV AC no ADC e so ruido (o piso medido fica em 4-6 mV).
        let sinal = |ac: f32| if ac < 20.0 { "SEM SINAL" } else { "lendo" };
        // So neste teste: desconta o piso de ruido do SCT (ruido e sinal somam em quadratura).
        let i_ac = (d.i_ac_mv * d.i_ac_mv - RUIDO_I_MV * RUIDO_I_MV).max(0.0).sqrt();
        println!("----------------------------------------------");
        println!(
            "SCT-013 (corrente, D35) = {:.3} A    | ADC: {:.2} mV AC (bruto {:.2}), bias {:.0} mV  [{}]",
            i_ac * sensores::sct013::FATOR_CORRENTE, i_ac, d.i_ac_mv, d.i_dc_mv, sinal(d.i_ac_mv)
        );
        println!(
            "ZMPT101B (tensao, D32)  = {:.1} V     | ADC: {:.1} mV AC, bias {:.0} mV  [{}]",
            l.vrms, d.v_ac_mv, d.v_dc_mv, sinal(d.v_ac_mv)
        );
        println!("Potencia = {:.1} W   aparente = {:.1} VA   FP = {:.2}", l.p, l.s, l.fp);
        std::thread::sleep(Duration::from_secs(2));
    }
}
