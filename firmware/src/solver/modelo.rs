//! Helpers de parametros do modelo (topicos 1-2 do PDF).
//!
//! Mantem a notacao do PDF acessivel para o solver e os testes.

use crate::solver::tipos::TomadaInput;

/// Coeficiente do termo de conforto preditivo da objetivo para `x_{i,OFF}`:
/// `alpha * Pi_{i,t} * W_i`.
pub fn coef_conforto_off(alpha: f64, t: &TomadaInput) -> f64 {
    alpha * t.pi * t.w_i
}

/// `1[A_{i,t}]` como numero (1.0 permitido, 0.0 proibido) — usado em R5.
pub fn a_it_num(t: &TomadaInput) -> f64 {
    if t.a_it {
        1.0
    } else {
        0.0
    }
}

/// `1[U_i]` como numero — usado em R3.
pub fn u_i_num(t: &TomadaInput) -> f64 {
    if t.u_i {
        1.0
    } else {
        0.0
    }
}

/// `1[F_i]` como numero — usado em R4.
pub fn f_i_num(t: &TomadaInput) -> f64 {
    if t.f_i {
        1.0
    } else {
        0.0
    }
}
