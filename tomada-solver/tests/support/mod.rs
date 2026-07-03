//! Carregador do dataset sintetico do orientador (host-only).
//!
//! O CSV e gerado por `gerador_dataset_iot.py` (seed 42): 7 dias, amostragem 5 min,
//! 4 aparelhos => 2016 instantes x 4 tomadas. Cada `timestamp` vira um `SolverInput`.
//! O `p_max` NAO esta no CSV (casa unica, `carga_fixa = 0`); e parametro da varredura.
//!
//! Mapeamento das colunas (contrato em `tipos.rs::TomadaInput`):
//!   C_ON->c_on, C_STD->c_std, U_i->u_i, F_i->f_i, Pi_it->pi, W_i->w_i, A_it->a_it.
//! `P_atual`/`T_ocioso` sao emulacao do sensor; NAO entram no solver (so baseline/relatorio).

#![allow(dead_code)] // nem todo consumidor (teste vs. exemplo) usa todos os campos/funcoes.

use std::path::PathBuf;

use serde::Deserialize;
use tomada_solver::{Pesos, SolverInput, TomadaInput};

/// Linha crua do CSV (uma tomada num instante).
#[derive(Debug, Deserialize)]
struct Linha {
    timestamp: String,
    dispositivo: String,
    #[serde(rename = "C_ON")]
    c_on: f64,
    #[serde(rename = "C_STD")]
    c_std: f64,
    #[serde(rename = "W_i")]
    w_i: f64,
    #[serde(rename = "P_atual")]
    p_atual: f64,
    #[serde(rename = "T_ocioso")]
    t_ocioso: f64,
    #[serde(rename = "U_i")]
    u_i: u8,
    #[serde(rename = "F_i")]
    f_i: u8,
    #[serde(rename = "Pi_it")]
    pi: f64,
    #[serde(rename = "A_it")]
    a_it: u8,
    cenario_injetado: String,
}

/// Uma tomada num instante: o input do solver + metadados do gabarito/sensor.
#[derive(Debug, Clone)]
pub struct Tomada {
    pub dispositivo: String,
    /// `cenario_injetado`: GABARITO (`Normal` | `Uso_Fora_Agendamento` | `Consumo_Fantasma_Ativo`).
    pub cenario: String,
    /// `P_atual` (W) medido/emulado pelo sensor — usado so no baseline do relatorio.
    pub p_atual: f64,
    /// Tempo ocioso acumulado (min) — emulacao do sensor, fora do solver.
    pub t_ocioso: f64,
    /// Parametros que entram no modelo.
    pub input: TomadaInput,
}

/// Snapshot de um instante `t`: as 4 tomadas na ordem do CSV
/// (Geladeira, Roteador, TV_Sala, Ar_Cond).
#[derive(Debug, Clone)]
pub struct Instante {
    pub timestamp: String,
    pub hora: u8,
    pub tomadas: Vec<Tomada>,
}

impl Instante {
    /// Monta o `SolverInput` deste instante para um teto `p_max` (carga_fixa = 0).
    pub fn solver_input(&self, p_max: f64) -> SolverInput {
        SolverInput {
            p_max,
            hora: self.hora,
            pesos: Pesos::default(),
            carga_fixa: 0.0,
            tomadas: self.tomadas.iter().map(|t| t.input).collect(),
        }
    }
}

/// Caminho do CSV versionado, relativo a este crate.
pub fn caminho_csv() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
        .join("dataset_iot_energia.csv")
}

/// Hora 0..23 a partir de "YYYY-MM-DD HH:MM:SS".
fn hora_de(timestamp: &str) -> u8 {
    timestamp
        .get(11..13)
        .and_then(|h| h.parse().ok())
        .unwrap_or_else(|| panic!("timestamp invalido: {timestamp:?}"))
}

/// Le o CSV e agrupa por timestamp (o gerador ja emite as 4 tomadas contiguas e em ordem).
pub fn carregar() -> Vec<Instante> {
    let caminho = caminho_csv();
    let mut leitor = csv::Reader::from_path(&caminho)
        .unwrap_or_else(|e| panic!("abrindo {}: {e}", caminho.display()));

    let mut instantes: Vec<Instante> = Vec::new();
    for registro in leitor.deserialize() {
        let l: Linha = registro.expect("linha do CSV malformada");

        let tomada = Tomada {
            dispositivo: l.dispositivo,
            cenario: l.cenario_injetado,
            p_atual: l.p_atual,
            t_ocioso: l.t_ocioso,
            input: TomadaInput {
                c_std: l.c_std,
                c_on: l.c_on,
                u_i: l.u_i != 0,
                f_i: l.f_i != 0,
                pi: l.pi,
                w_i: l.w_i,
                a_it: l.a_it != 0,
            },
        };

        match instantes.last_mut() {
            Some(inst) if inst.timestamp == l.timestamp => inst.tomadas.push(tomada),
            _ => {
                let hora = hora_de(&l.timestamp);
                instantes.push(Instante { timestamp: l.timestamp, hora, tomadas: vec![tomada] });
            }
        }
    }

    instantes
}
