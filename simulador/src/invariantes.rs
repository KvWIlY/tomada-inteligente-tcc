//! Invariantes observaveis de fora do `decidir` (FR-019, research D8) e a regressao
//! plantada (SC-010). A R2 dentro de cada solve e a integralidade de `x` nao sao
//! observaveis pela interface do ciclo (FR-019b).

use crate::solver::Estado;

/// Regra que `--plantar` adultera.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Regra {
    R3,
    R4,
    R5,
}

impl Regra {
    pub fn nome(self) -> &'static str {
        match self {
            Regra::R3 => "r3",
            Regra::R4 => "r4",
            Regra::R5 => "r5",
        }
    }
}

/// O que a conferencia enxerga de uma `Decisao`. Com `--plantar` ela e adulterada numa
/// copia: o `Ciclo` e o anuncio seguem com a decisao verdadeira.
#[derive(Debug, Clone)]
pub struct Visao {
    pub estado: Estado,
    pub u_i: bool,
    pub f_i: bool,
    /// `A_it` do `config_passo`, o mesmo que o solver recebeu.
    pub a_it: bool,
    pub pen_uso: bool,
    pub pen_age: bool,
}

/// Adultera a primeira tomada onde a regra e plantavel. `true` se plantou.
pub fn plantar(regra: Regra, visoes: &mut [Visao]) -> bool {
    let alvo = match regra {
        Regra::R4 => visoes.iter().position(|v| v.f_i),
        Regra::R3 => visoes.iter().position(|v| v.u_i && v.estado == Estado::Off && v.pen_uso),
        Regra::R5 => visoes.iter().position(|v| v.estado == Estado::On && v.a_it && !v.pen_age),
    };
    let Some(i) = alvo else { return false };
    let v = &mut visoes[i];
    match regra {
        Regra::R4 => v.estado = Estado::Std,
        Regra::R3 => v.pen_uso = false,
        Regra::R5 => v.pen_age = true,
    }
    true
}

/// Primeira violacao do passo: `(indice da tomada, descricao)`. `None` para R1 com
/// contagem errada, que nao pertence a uma tomada.
pub fn conferir(visoes: &[Visao], n_tomadas: usize) -> Option<(Option<usize>, String)> {
    if visoes.len() != n_tomadas {
        return Some((None, format!("R1 violada - {} decisões para {} tomadas", visoes.len(), n_tomadas)));
    }
    for (i, v) in visoes.iter().enumerate() {
        if v.f_i && v.estado == Estado::Std {
            return Some((Some(i), "R4 violada - F_i ativo e estado STD".to_string()));
        }
        let uso_cortado = v.u_i && v.estado == Estado::Off;
        if uso_cortado && !v.pen_uso {
            return Some((Some(i), "R3 violada - U_i ativo e estado OFF sem evento penalidade_uso".to_string()));
        }
        if !uso_cortado && v.pen_uso {
            return Some((Some(i), format!(
                "R3 violada - evento penalidade_uso sem uso ativo cortado (U_i {}, estado {})",
                if v.u_i { "ativo" } else { "inativo" },
                v.estado.as_str()
            )));
        }
        let fora_do_horario = !v.a_it && v.estado == Estado::On;
        if fora_do_horario && !v.pen_age {
            return Some((Some(i), "R5 violada - estado ON fora do horário sem evento penalidade_age".to_string()));
        }
        if !fora_do_horario && v.pen_age {
            return Some((Some(i), format!(
                "R5 violada - evento penalidade_age sem ON fora do horário (A_it {}, estado {})",
                if v.a_it { "permitido" } else { "proibido" },
                v.estado.as_str()
            )));
        }
    }
    None
}
