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

use potencia::{media, rms_centrado, Leitura};

/// Amostras por janela de medicao.
pub const N_AMOSTRAS: usize = 1000;
/// Intervalo entre amostras (us). 1000 * 200us = 200 ms (~12 ciclos de 60 Hz).
pub const INTERVALO_US: u32 = 200;

/// Diagnostico bruto de uma janela, em **mV no ADC** (antes dos fatores de calibracao).
/// Nao participa da decisao — serve so para caracterizar o piso de ruido no console e
/// escolher o limiar de gate. `*_dc` = media (bias DC); `*_ac` = RMS centrado (componente AC).
#[derive(Debug, Clone, Copy)]
pub struct Diag {
    pub v_dc_mv: f32,
    pub v_ac_mv: f32,
    pub i_dc_mv: f32,
    pub i_ac_mv: f32,
}

/// Coleta `N_AMOSTRAS` de tensao e corrente (interleaved) e devolve a `Leitura` + o
/// `Diag` bruto (mV) da mesma janela. `ler_v_mv`/`ler_i_mv` retornam uma amostra do canal em mV.
pub fn medir<FV, FI>(mut ler_v_mv: FV, mut ler_i_mv: FI) -> (Leitura, Diag)
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
    let diag = Diag {
        v_dc_mv: media(&v),
        v_ac_mv: rms_centrado(&v),
        i_dc_mv: media(&i),
        i_ac_mv: rms_centrado(&i),
    };
    let vrms = zmpt101b::vrms_volts(&v);
    let irms = sct013::irms_amps(&i);
    (potencia::leitura(vrms, irms), diag)
}
