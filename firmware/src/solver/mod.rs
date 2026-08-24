//! Modelo MILP de otimizacao de tomadas inteligentes.
//!
//! Implementa os topicos 1-4 do `review_modelo.pdf` SEM alteracoes:
//! conjuntos/parametros, variaveis de decisao binarias, funcao objetivo e R1-R5.
//!
//! Logica PURA (sem I/O, sem hardware): entra `SolverInput`, sai `SolverOutput`.
//! Deterministico => reproduzivel (Constituicao, Principio VII).
//!
//! Era o crate `tomada-solver`, absorvido para dentro do firmware em 2026-08-24.
//! Os modulos continuam sem tocar `esp-idf`: o I/O mora nos modulos irmaos
//! (`comunicacao/`, `persistencia/`, `sensores/`), que chamam estas funcoes.

pub mod coordenacao;
pub mod modelo;
pub mod pareamento;
pub mod portal;
pub mod relogio;
pub mod replay;
pub mod retencao;
pub mod solver;
pub mod sync;
pub mod tipos;
pub mod uso_agora;

pub use solver::resolver;
pub use tipos::{Estado, Pesos, SolverInput, SolverOutput, TipoPenalidade, TomadaInput};
