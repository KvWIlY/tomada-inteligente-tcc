//! Sensor de TENSAO ZMPT101B (GPIO 32 / ADC1_CH4).
//!
//! O ZMPT101B entrega uma tensao AC proporcional a tensao da rede, com offset DC
//! ajustavel no trimpot da placa. Lemos amostras em mV no ADC, removemos o DC (RMS
//! centrado) e multiplicamos por um FATOR de calibracao para obter Vrms da rede.
//!
//! >>> CALIBRACAO (ajuste no SEU hardware): aplique uma tensao conhecida (ex.: 127 V ou
//! 220 V medida com multimetro) e ajuste `FATOR_TENSAO` ate `vrms_volts` bater. <<<

use crate::sensores::potencia::rms_centrado;

/// Volts-de-rede por mV-RMS-no-ADC. Aproximado (2026-11-01): 127 V NOMINAIS da rede, sem
/// multimetro, davam 139,2 mV com o trimpot atual. Mexeu no trimpot, recalibre.
pub const FATOR_TENSAO: f32 = 0.912;

/// Vrms da rede (V) a partir das amostras em mV do ADC.
pub fn vrms_volts(amostras_mv: &[f32]) -> f32 {
    rms_centrado(amostras_mv) * FATOR_TENSAO
}
