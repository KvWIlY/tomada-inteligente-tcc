//! Sensores: amostragem RMS de tensao (ZMPT101B) e corrente (SCT-013) e derivacao de
//! potencia (research §3).
//!
//! A leitura do ADC fica DESACOPLADA do hardware: `medir` recebe closures que devolvem
//! uma amostra em mV. `main` cria o `AdcDriver`/`AdcChannelDriver` (esp-idf-hal oneshot)
//! e passa `|| ch.read().unwrap_or(0) as f32`. Assim a matematica (RMS/P) e host-pura e
//! testavel, e o unico ponto dependente da API do ADC sao ~4 linhas em `main`.

pub mod potencia;
pub mod sct013;
pub mod zmpt101b;

use potencia::Leitura;

/// Amostras por janela de medicao.
pub const N_AMOSTRAS: usize = 1000;
/// Intervalo entre amostras (us). 1000 * 200us = 200 ms (~12 ciclos de 60 Hz).
pub const INTERVALO_US: u32 = 200;

/// Coleta `N_AMOSTRAS` de tensao e corrente (interleaved) e devolve a `Leitura`.
/// `ler_v_mv`/`ler_i_mv` retornam uma amostra do canal em mV.
pub fn medir<FV, FI>(mut ler_v_mv: FV, mut ler_i_mv: FI) -> Leitura
where
    FV: FnMut() -> f32,
    FI: FnMut() -> f32,
{
    let mut v = Vec::with_capacity(N_AMOSTRAS);
    let mut i = Vec::with_capacity(N_AMOSTRAS);
    for _ in 0..N_AMOSTRAS {
        v.push(ler_v_mv());
        i.push(ler_i_mv());
        // Busy-delay de ROM: sempre disponivel via esp-idf-sys, sem trait de delay.
        unsafe { esp_idf_svc::sys::esp_rom_delay_us(INTERVALO_US) };
    }
    let vrms = zmpt101b::vrms_volts(&v);
    let irms = sct013::irms_amps(&i);
    potencia::leitura(vrms, irms)
}
