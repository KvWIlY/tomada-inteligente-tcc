//! Helpers de parametros do modelo (topicos 1-2 do PDF).
//!
//! Mantem a notacao do PDF acessivel para o solver e os testes.

use crate::solver::tipos::TomadaInput;

/// Coeficiente do termo de conforto preditivo da objetivo para `x_{i,OFF}`:
/// `alpha * W_i`. O `Pi_{i,t}` do PDF saiu do codigo em 2026-09-14 (valia 0.5 fixo);
/// o `alpha` default ja carrega esse fator, ver `Pesos::default`.
pub fn coef_conforto_off(alpha: f64, t: &TomadaInput) -> f64 {
    alpha * t.w_i
}

