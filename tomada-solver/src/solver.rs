//! Solver MILP: funcao objetivo + R1-R5 (topico 4 do PDF) com integralidade binaria
//! via branch-and-bound sobre a relaxacao LP do `minilp` (research.md §1).
//!
//! Garantias (contracts/solver-api.md):
//!  - sempre factivel (folgas y_uso/y_age + R2 saturada a >= 0);
//!  - x_{i,s} sempre inteiro (B&B);
//!  - deterministico (desempate estavel por indice).

use minilp::{ComparisonOp, OptimizationDirection, Problem, Variable};

use crate::modelo::{a_it_num, coef_conforto_off, f_i_num, u_i_num};
use crate::tipos::{
    modelo_consumo, Estado, Penalidade, SolverInput, SolverOutput, TipoPenalidade,
};

const EPS: f64 = 1e-6;

/// Solucao de um no do B&B (relaxacao LP resolvida).
struct NoSol {
    obj: f64,
    x: Vec<f64>,      // 3*n valores das variaveis x_{i,s}
    y_uso: Vec<f64>,  // n
    y_age: Vec<f64>,  // n
}

/// Resolve o modelo para o snapshot dado. Sempre retorna uma solucao factivel.
pub fn resolver(entrada: &SolverInput) -> SolverOutput {
    let n = entrada.tomadas.len();

    if n == 0 {
        return SolverOutput { estados: vec![], z: 0.0, penalidades: vec![], factivel: true };
    }

    let mut fixado: Vec<Option<bool>> = vec![None; 3 * n];
    let mut melhor: Option<NoSol> = None;
    bb(entrada, &mut fixado, &mut melhor);

    let sol = melhor.expect("modelo sempre tem solucao factivel (todos OFF)");

    // Estado escolhido por tomada = o s com x ~ 1.
    let mut estados = Vec::with_capacity(n);
    for i in 0..n {
        let base = i * 3;
        let mut melhor_s = 0usize;
        let mut melhor_v = sol.x[base];
        for s in 1..3 {
            if sol.x[base + s] > melhor_v {
                melhor_v = sol.x[base + s];
                melhor_s = s;
            }
        }
        estados.push(Estado::TODOS[melhor_s]);
    }

    // Penalidades ativadas (folgas > 0).
    let mut penalidades = Vec::new();
    for i in 0..n {
        if sol.y_uso[i] > EPS {
            penalidades.push(Penalidade {
                tomada_idx: i,
                tipo: TipoPenalidade::Uso,
                valor: sol.y_uso[i],
            });
        }
        if sol.y_age[i] > EPS {
            penalidades.push(Penalidade {
                tomada_idx: i,
                tipo: TipoPenalidade::Agendamento,
                valor: sol.y_age[i],
            });
        }
    }

    SolverOutput { estados, z: sol.obj, penalidades, factivel: true }
}

/// Branch-and-bound recursivo. Minimiza; poda por bound (relaxacao = limite inferior).
fn bb(entrada: &SolverInput, fixado: &mut [Option<bool>], melhor: &mut Option<NoSol>) {
    let no = match resolver_relaxacao(entrada, fixado) {
        Some(no) => no,
        None => return, // ramo infactivel
    };

    // Poda: se o limite inferior nao supera o melhor inteiro ja achado, abandona.
    if let Some(b) = melhor {
        if no.obj >= b.obj - EPS {
            return;
        }
    }

    // Variavel x mais fracionaria (mais proxima de 0.5) ainda livre.
    let mut alvo: Option<usize> = None;
    let mut menor_dist = 0.5 - EPS;
    for k in 0..fixado.len() {
        if fixado[k].is_some() {
            continue;
        }
        let v = no.x[k];
        let frac = (v - v.round()).abs();
        if frac > EPS {
            let dist = (v - 0.5).abs();
            if dist < menor_dist {
                menor_dist = dist;
                alvo = Some(k);
            }
        }
    }

    match alvo {
        None => {
            // Solucao inteira: candidata.
            let melhora = match melhor {
                Some(b) => no.obj < b.obj - EPS,
                None => true,
            };
            if melhora {
                *melhor = Some(no);
            }
        }
        Some(k) => {
            // Ramifica fixando x_k = 0 e x_k = 1.
            for val in [false, true] {
                fixado[k] = Some(val);
                bb(entrada, fixado, melhor);
            }
            fixado[k] = None;
        }
    }
}

/// Monta e resolve a relaxacao LP com os limites de ramificacao em `fixado`.
fn resolver_relaxacao(entrada: &SolverInput, fixado: &[Option<bool>]) -> Option<NoSol> {
    let n = entrada.tomadas.len();
    let alpha = entrada.pesos.alpha;
    let beta = entrada.pesos.beta;
    let gamma = entrada.pesos.gamma;

    let mut p = Problem::new(OptimizationDirection::Minimize);

    // Variaveis x_{i,s} (layout i*3 + s).
    let mut xv: Vec<Variable> = Vec::with_capacity(3 * n);
    for (i, t) in entrada.tomadas.iter().enumerate() {
        for s in 0..3 {
            let estado = Estado::TODOS[s];
            let mut coef = modelo_consumo(estado, t); // 1o termo: consumo direto
            if estado == Estado::Off {
                coef += coef_conforto_off(alpha, t); // 2o termo: conforto preditivo
            }
            let bounds = match fixado[i * 3 + s] {
                Some(true) => (1.0, 1.0),
                Some(false) => (0.0, 0.0),
                None => (0.0, 1.0),
            };
            xv.push(p.add_var(coef, bounds));
        }
    }

    // Variaveis de folga (3o termo: beta*y_uso + gamma*y_age).
    let mut y_uso: Vec<Variable> = Vec::with_capacity(n);
    let mut y_age: Vec<Variable> = Vec::with_capacity(n);
    for _ in 0..n {
        y_uso.push(p.add_var(beta, (0.0, f64::INFINITY)));
        y_age.push(p.add_var(gamma, (0.0, f64::INFINITY)));
    }

    // R1: unicidade de estado — sum_s x_{i,s} = 1.
    for i in 0..n {
        p.add_constraint(
            &[(xv[i * 3], 1.0), (xv[i * 3 + 1], 1.0), (xv[i * 3 + 2], 1.0)],
            ComparisonOp::Eq,
            1.0,
        );
    }

    // R4 (hard): x_{i,STD} <= 1 - F_i.
    for (i, t) in entrada.tomadas.iter().enumerate() {
        p.add_constraint(&[(xv[i * 3 + 1], 1.0)], ComparisonOp::Le, 1.0 - f_i_num(t));
    }

    // R3 (soft): x_{i,OFF} - y_uso_i <= 1 - U_i.
    for (i, t) in entrada.tomadas.iter().enumerate() {
        p.add_constraint(
            &[(xv[i * 3], 1.0), (y_uso[i], -1.0)],
            ComparisonOp::Le,
            1.0 - u_i_num(t),
        );
    }

    // R5 (soft): x_{i,ON} - y_age_i <= A_{i,t}.
    for (i, t) in entrada.tomadas.iter().enumerate() {
        p.add_constraint(
            &[(xv[i * 3 + 2], 1.0), (y_age[i], -1.0)],
            ComparisonOp::Le,
            a_it_num(t),
        );
    }

    // R2 (hard): sum_i (C_std*x_std + C_on*x_on) <= P_max - carga_fixa (>= 0).
    let mut r2: Vec<(Variable, f64)> = Vec::with_capacity(2 * n);
    for (i, t) in entrada.tomadas.iter().enumerate() {
        r2.push((xv[i * 3 + 1], t.c_std));
        r2.push((xv[i * 3 + 2], t.c_on));
    }
    let teto = (entrada.p_max - entrada.carga_fixa).max(0.0);
    p.add_constraint(&r2, ComparisonOp::Le, teto);

    let sol = p.solve().ok()?;

    let x: Vec<f64> = xv.iter().map(|v| sol[*v]).collect();
    let y_uso_v: Vec<f64> = y_uso.iter().map(|v| sol[*v]).collect();
    let y_age_v: Vec<f64> = y_age.iter().map(|v| sol[*v]).collect();

    Some(NoSol { obj: sol.objective(), x, y_uso: y_uso_v, y_age: y_age_v })
}
