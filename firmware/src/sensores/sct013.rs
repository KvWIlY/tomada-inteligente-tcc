//! Sensor de CORRENTE SCT-013 20A/1V (GPIO 35 / ADC1_CH7).
//!
//! A variante 20A/1V ja entrega TENSAO (nao corrente): 1 V de saida <-> 20 A. A saida e
//! AC centrada num bias (divisor para o meio da faixa do ADC). Removemos o DC (RMS
//! centrado) e convertemos mV-RMS -> A com o fator nominal (20 A / 1000 mV = 0.02 A/mV),
//! ajustavel por calibracao do divisor real.
//!
//! >>> CALIBRACAO: com uma carga de corrente conhecida (ex.: 1 A), ajuste
//! `FATOR_CORRENTE` ate `irms_amps` bater. <<<

use crate::sensores::potencia::rms_centrado;

/// Amperes por mV-RMS-no-ADC. Nominal 20 A / 1000 mV = 0.02. PLACEHOLDER - calibrar.
pub const FATOR_CORRENTE: f32 = 0.02;

/// Irms (A) a partir das amostras em mV do ADC.
pub fn irms_amps(amostras_mv: &[f32]) -> f32 {
    rms_centrado(amostras_mv) * FATOR_CORRENTE
}
