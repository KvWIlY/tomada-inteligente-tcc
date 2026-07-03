//! Modelo MILP de otimizacao de tomadas inteligentes.
//!
//! Implementa os topicos 1-4 do `review_modelo.pdf` SEM alteracoes:
//! conjuntos/parametros, variaveis de decisao binarias, funcao objetivo e R1-R5.
//!
//! Crate puro (sem I/O, sem hardware): entra `SolverInput`, sai `SolverOutput`.
//! Deterministico => reproduzivel (Constituicao, Principio VII).
//! Testavel no host: `cargo test -p tomada-solver`.

pub mod coordenacao;
pub mod modelo;
pub mod solver;
pub mod sync;
pub mod tipos;

pub use solver::resolver;
pub use tipos::{
    Estado, Penalidade, Pesos, SolverInput, SolverOutput, TipoPenalidade, TomadaInput,
};
