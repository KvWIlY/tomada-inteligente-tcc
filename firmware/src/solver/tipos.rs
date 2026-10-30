//! Tipos do modelo (contrato em `contracts/solver-api.md`).

use serde::{Deserialize, Serialize};

/// Conjunto de estados operacionais `S = {OFF, STD, ON}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Estado {
    Off = 0,
    Std = 1,
    On = 2,
}

impl Estado {
    /// Indice 0..2 (alinha com o layout das variaveis do solver).
    pub fn idx(self) -> usize {
        self as usize
    }

    /// Rotulo textual (usado na serializacao para o Supabase).
    pub fn as_str(self) -> &'static str {
        match self {
            Estado::Off => "OFF",
            Estado::Std => "STD",
            Estado::On => "ON",
        }
    }
}

/// Pesos da funcao objetivo. Convencao do PDF: `beta, gamma >> 1`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Pesos {
    /// Peso do termo de conforto preditivo (`alpha`).
    pub alpha: f64,
    /// Penalidade de violar uso ativo (`beta`, R3).
    pub beta: f64,
    /// Penalidade de violar agendamento (`gamma`, R5).
    pub gamma: f64,
}

impl Default for Pesos {
    fn default() -> Self {
        // beta, gamma >> 1 para dominar o consumo em Watts.
        // alpha = 0.5 absorve o `Pi_{i,t}` que saiu em 2026-09-14: nada no sistema o
        // calculava e ele valia 0.5 fixo, entao `alpha*Pi*W` virou `alpha*W` com o
        // mesmo numero. Nenhuma decisao muda.
        Pesos { alpha: 0.5, beta: 1.0e6, gamma: 1.0e6 }
    }
}

/// Parametros de uma tomada `i` para o instante `t` (snapshot do ciclo).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TomadaInput {
    /// `C_{i,STD}` (W). `C_{i,OFF} = 0` implicito.
    pub c_std: f64,
    /// `C_{i,ON}` (W).
    pub c_on: f64,
    /// `U_i`: uso ativo imediato (R3).
    pub u_i: bool,
    /// `F_i`: consumo fantasma detectado (R4).
    pub f_i: bool,
    /// `W_i`: peso de criticidade (M se critico).
    pub w_i: f64,
    /// `A_{i,t}`: horario permitido na hora corrente (R5).
    pub a_it: bool,
}

/// Snapshot multi-tomada para o instante `t`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolverInput {
    /// `P_max` (W), teto global (R2).
    pub p_max: f64,
    /// Hora corrente 0..23 (ja usada para indexar `a_it` no firmware).
    pub hora: u8,
    /// Pesos da objetivo.
    pub pesos: Pesos,
    /// Carga fixa ja comprometida (peers ausentes via ESP-NOW; W). Subtraida de `p_max`.
    #[serde(default)]
    pub carga_fixa: f64,
    /// Tomadas a otimizar.
    pub tomadas: Vec<TomadaInput>,
}

/// Tipo de folga ativada.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TipoPenalidade {
    /// `y_uso` (R3): OFF forcado durante uso ativo.
    Uso,
    /// `y_age` (R5): ON fora do horario agendado.
    Agendamento,
}

/// Penalidade ativada na solucao (folga > 0).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Penalidade {
    pub tomada_idx: usize,
    pub tipo: TipoPenalidade,
    pub valor: f64,
}

/// Resultado da otimizacao.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolverOutput {
    /// `x_{i,s}` resolvido: um estado por tomada.
    pub estados: Vec<Estado>,
    /// Valor da funcao objetivo `Z`.
    pub z: f64,
    /// Folgas `y_uso`/`y_age` ativadas.
    pub penalidades: Vec<Penalidade>,
    /// Sempre `true` por construcao (folgas); exposto p/ asercao.
    pub factivel: bool,
}

impl SolverOutput {
    /// Consumo total dos estados escolhidos (sem a carga fixa).
    pub fn consumo(&self, entrada: &SolverInput) -> f64 {
        self.estados
            .iter()
            .zip(&entrada.tomadas)
            .map(|(e, t)| modelo_consumo(*e, t))
            .sum()
    }
}

/// Consumo `C_{i,s}` de uma tomada em um estado.
pub(crate) fn modelo_consumo(estado: Estado, t: &TomadaInput) -> f64 {
    match estado {
        Estado::Off => 0.0,
        Estado::Std => t.c_std,
        Estado::On => t.c_on,
    }
}
