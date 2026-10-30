//! Solver exato do modelo (funcao objetivo + R1-R5, topico 4 do PDF) por busca em
//! profundidade com poda. Substituiu o branch-and-bound sobre a relaxacao LP do `minilp`
//! em 2026-10-27 (revisao do orientador, recomendacao 2b): o modelo e o mesmo, so o
//! metodo de resolucao mudou.
//!
//! Por que da para enumerar: com R3b e R4, cada tomada tem no maximo duas opcoes.
//! Em uso: ON (R3b proibe STD) ou OFF (R3, folga `y_uso`). Ociosa: STD ou OFF, e so OFF
//! se `F_i` (R4). R1 fica garantida por construcao e R2 e a unica restricao que liga as
//! tomadas: o problema e uma mochila 0/1 ("manter" ou "cortar" cada tomada).
//!
//! Garantias (contracts/solver-api.md):
//!  - sempre factivel (todas em OFF cabe em qualquer teto >= 0);
//!  - otimo exato enquanto a busca couber em `LIMITE_NOS`; acima disso, a melhor solucao
//!    achada, que nunca e pior que a gulosa (regra de prioridade) usada como partida;
//!  - deterministico, com desempate explicito: entre solucoes de mesmo custo, mantem
//!    ligada a tomada de menor indice (o `decidir` ordena por MAC). Em particular, quando
//!    manter e cortar custam o mesmo, a tomada fica ligada.

use crate::solver::modelo::coef_conforto_off;
use crate::solver::tipos::{
    Estado, Penalidade, SolverInput, SolverOutput, TipoPenalidade, TomadaInput,
};

/// Tolerancia de comparacao de custos (W). A soma e feita em f64 sobre no maximo dezenas
/// de termos de ate `alpha*M`, entao o erro de arredondamento fica muito abaixo disso.
const EPS: f64 = 1e-6;

/// Teto de nos visitados pela busca. Com teto folgado a busca nem roda (caminho rapido);
/// com teto ativo, a poda deixa o numero de nos muito abaixo de `2^n` nos casos medidos.
/// ponytail: limite fixo de nos, trocar por prazo em us se o ESP32 medir acima do ciclo.
pub const LIMITE_NOS: u32 = 200_000;

/// As duas opcoes de uma tomada ja traduzidas para custo na funcao objetivo.
struct Opcao {
    /// Estado ligado possivel (ON em uso, STD ociosa) ou `None` se so OFF e permitido.
    manter: Option<Estado>,
    /// Potencia somada na R2 ao manter.
    p: f64,
    /// Custo ao manter: consumo + `gamma` se ON fora do horario (R5).
    custo_manter: f64,
    /// Custo ao cortar: `alpha*W_i` + `beta` se estava em uso (R3).
    custo_off: f64,
}

impl Opcao {
    fn de(t: &TomadaInput, entrada: &SolverInput) -> Self {
        let pesos = entrada.pesos;
        let custo_off = coef_conforto_off(pesos.alpha, t) + if t.u_i { pesos.beta } else { 0.0 };
        let (manter, p, custo_manter) = if t.u_i {
            let gamma = if t.a_it { 0.0 } else { pesos.gamma };
            (Some(Estado::On), t.c_on, t.c_on + gamma)
        } else if !t.f_i {
            (Some(Estado::Std), t.c_std, t.c_std)
        } else {
            (None, 0.0, 0.0)
        };
        Opcao { manter, p, custo_manter, custo_off }
    }

    /// Quanto manter economiza na objetivo em relacao a cortar.
    fn ganho(&self) -> f64 {
        self.custo_off - self.custo_manter
    }
}

/// Resolve o modelo para o snapshot dado. Sempre retorna uma solucao factivel.
pub fn resolver(entrada: &SolverInput) -> SolverOutput {
    let n = entrada.tomadas.len();
    let opcoes: Vec<Opcao> = entrada.tomadas.iter().map(|t| Opcao::de(t, entrada)).collect();
    let teto = (entrada.p_max - entrada.carga_fixa).max(0.0);

    // Candidatas a ficar ligadas: as que podem e cujo manter nao custa mais que cortar.
    let cand: Vec<usize> =
        (0..n).filter(|&i| opcoes[i].manter.is_some() && opcoes[i].ganho() > -EPS).collect();

    let mut manter = vec![false; n];
    let p_cand: f64 = cand.iter().map(|&i| opcoes[i].p).sum();
    if p_cand <= teto + EPS {
        // Caminho rapido (teto folgado): cada tomada fica na sua opcao mais barata.
        for &i in &cand {
            manter[i] = true;
        }
    } else {
        // Partida gulosa = regra de prioridade: maior ganho primeiro, desempate por indice.
        let mut ordem = cand.clone();
        ordem.sort_by(|&a, &b| opcoes[b].ganho().total_cmp(&opcoes[a].ganho()).then(a.cmp(&b)));
        let mut usado = 0.0;
        for &i in &ordem {
            if usado + opcoes[i].p <= teto + EPS {
                manter[i] = true;
                usado += opcoes[i].p;
            }
        }
        let ganho_guloso: f64 = cand.iter().filter(|&&i| manter[i]).map(|&i| opcoes[i].ganho()).sum();

        // Busca exata, em ordem de indice, "manter" antes de "cortar"; so troca a melhor
        // solucao por uma estritamente melhor, o que fixa o desempate descrito no topo.
        let mut resto = vec![0.0; cand.len() + 1];
        for k in (0..cand.len()).rev() {
            resto[k] = resto[k + 1] + opcoes[cand[k]].ganho().max(0.0);
        }
        let mut busca = Busca {
            opcoes: &opcoes,
            cand: &cand,
            resto: &resto,
            teto,
            atual: vec![false; cand.len()],
            melhor: None,
            // Um pouco abaixo da gulosa: a primeira folha otima na ordem da busca vence,
            // mesmo quando a gulosa ja era otima.
            melhor_ganho: ganho_guloso - 2.0 * EPS,
            nos: 0,
        };
        busca.dfs(0, 0.0, 0.0);
        if let Some(sel) = busca.melhor {
            manter = vec![false; n];
            for (k, &i) in cand.iter().enumerate() {
                manter[i] = sel[k];
            }
        }
    }

    let mut estados = Vec::with_capacity(n);
    let mut penalidades = Vec::new();
    let mut z = 0.0;
    for (i, (o, t)) in opcoes.iter().zip(&entrada.tomadas).enumerate() {
        if manter[i] {
            estados.push(o.manter.expect("so candidatas sao mantidas"));
            z += o.custo_manter;
            if t.u_i && !t.a_it {
                penalidades.push(Penalidade { tomada_idx: i, tipo: TipoPenalidade::Agendamento, valor: 1.0 });
            }
        } else {
            estados.push(Estado::Off);
            z += o.custo_off;
            if t.u_i {
                penalidades.push(Penalidade { tomada_idx: i, tipo: TipoPenalidade::Uso, valor: 1.0 });
            }
        }
    }

    SolverOutput { estados, z, penalidades, factivel: true }
}

struct Busca<'a> {
    opcoes: &'a [Opcao],
    cand: &'a [usize],
    /// `resto[k]` = soma dos ganhos positivos das candidatas `k..`: limite superior do que
    /// ainda da para ganhar.
    resto: &'a [f64],
    teto: f64,
    atual: Vec<bool>,
    /// `None` se o limite de nos estourou antes da primeira folha (fica a gulosa).
    melhor: Option<Vec<bool>>,
    melhor_ganho: f64,
    nos: u32,
}

impl Busca<'_> {
    fn dfs(&mut self, k: usize, p: f64, ganho: f64) {
        self.nos += 1;
        if self.nos > LIMITE_NOS || ganho + self.resto[k] <= self.melhor_ganho + EPS {
            return; // estourou o limite ou este ramo nao pode ser estritamente melhor
        }
        if k == self.cand.len() {
            self.melhor_ganho = ganho;
            self.melhor = Some(self.atual.clone());
            return;
        }
        let o = &self.opcoes[self.cand[k]];
        if p + o.p <= self.teto + EPS {
            self.atual[k] = true;
            self.dfs(k + 1, p + o.p, ganho + o.ganho());
            self.atual[k] = false;
        }
        self.dfs(k + 1, p, ganho);
    }
}
