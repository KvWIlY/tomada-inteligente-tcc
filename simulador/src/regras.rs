//! Regras de comparacao com o MILP (parecer de 2026-10-22). Trocam SO o `resolver` que o
//! `Ciclo` do firmware chama: medicao, `U_i`/`F_i`, calibracao, "usar agora", demanda
//! pendente, comando manual subordinado ao `P_max` e a coordenacao pelo anuncio do passo
//! anterior ficam os mesmos. O firmware nao e tocado: o `main.rs` monta o modulo `solver`
//! com os fontes dele e aponta o `resolver` para ca.

use std::sync::atomic::{AtomicU8, Ordering};

use crate::solver::tipos::{modelo_consumo, Estado, Penalidade, SolverInput, SolverOutput, TipoPenalidade};

/// Quem decide o estado das tomadas: `[casa] decisao` do caso.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Modo {
    /// O MILP do firmware, sem mudanca.
    #[default]
    Milp,
    /// Limiar + temporizador por tomada (Gheorghe et al. 2024): em uso fica ligada, ociosa
    /// fica em espera, fantasma (`F_i`, ociosa ha mais de `t_limite`) e cortada. Ignora o teto.
    Regra,
    /// A `Regra` e, se a soma passar do teto, corta as em uso ate caber: menor `W_i`
    /// primeiro, a de maior consumo no empate. Ociosa nao entra: a espera nao resolve
    /// excesso, e a que tem "usar agora" aberto le 0 W com o rele aberto e pareceria ociosa.
    Prioridade,
    /// O MILP do firmware com informacao perfeita (revisao do orientador, 4.3): cada tomada
    /// ve as outras no passo ATUAL, e nao pelo anuncio do anterior. Todas resolvem a mesma
    /// entrada, o que equivale a um unico solver para a casa. A passada extra mora no `laco`.
    Centralizado,
}

impl Modo {
    pub fn as_str(self) -> &'static str {
        match self {
            Modo::Milp => "milp",
            Modo::Regra => "regra",
            Modo::Prioridade => "prioridade",
            Modo::Centralizado => "centralizado",
        }
    }
}

// ponytail: global do processo, uma rodada por processo; `verificar` e `varredura` rodam
// sempre com o MILP. Passar o modo pelo `Ciclo` exigiria mexer no firmware.
static MODO: AtomicU8 = AtomicU8::new(0);

pub fn definir(m: Modo) {
    MODO.store(m as u8, Ordering::Relaxed);
}

pub fn centralizado() -> bool {
    MODO.load(Ordering::Relaxed) == Modo::Centralizado as u8
}

pub fn resolver(e: &SolverInput) -> SolverOutput {
    match MODO.load(Ordering::Relaxed) {
        1 => regra(e, false),
        2 => regra(e, true),
        _ => crate::solver::solver::resolver(e),
    }
}

fn regra(e: &SolverInput, teto: bool) -> SolverOutput {
    let t = &e.tomadas;
    let mut estados: Vec<Estado> = t
        .iter()
        .map(|x| if x.u_i { Estado::On } else if x.f_i { Estado::Off } else { Estado::Std })
        .collect();
    if teto {
        let limite = (e.p_max - e.carga_fixa).max(0.0);
        let mut consumo: f64 = estados.iter().zip(t).map(|(s, x)| modelo_consumo(*s, x)).sum();
        let mut ordem: Vec<usize> = (0..t.len()).filter(|&i| estados[i] == Estado::On).collect();
        ordem.sort_by(|&a, &b| {
            t[a].w_i
                .total_cmp(&t[b].w_i)
                .then(t[b].c_on.total_cmp(&t[a].c_on))
                .then(a.cmp(&b))
        });
        let mut cortadas = Vec::new();
        for i in ordem {
            if consumo <= limite + 1e-6 {
                break;
            }
            consumo -= t[i].c_on;
            estados[i] = Estado::Off;
            cortadas.push(i);
        }
        // Cortar em ordem pode passar do necessario: devolve, da mais importante para a menos,
        // as que voltam a caber. Sem isto o comando manual do `Ciclo` religaria a cortada.
        for &i in cortadas.iter().rev() {
            if consumo + t[i].c_on <= limite + 1e-6 {
                consumo += t[i].c_on;
                estados[i] = Estado::On;
            }
        }
    }
    // As mesmas penalidades que o MILP sinalizaria, para os eventos e a conferencia da R3/R5.
    let mut penalidades = Vec::new();
    for (i, (s, x)) in estados.iter().zip(t).enumerate() {
        if x.u_i && *s == Estado::Off {
            penalidades.push(Penalidade { tomada_idx: i, tipo: TipoPenalidade::Uso, valor: 1.0 });
        }
        if !x.a_it && *s == Estado::On {
            penalidades.push(Penalidade { tomada_idx: i, tipo: TipoPenalidade::Agendamento, valor: 1.0 });
        }
    }
    let z = estados.iter().zip(t).map(|(s, x)| modelo_consumo(*s, x)).sum();
    SolverOutput { estados, z, penalidades, factivel: true }
}
