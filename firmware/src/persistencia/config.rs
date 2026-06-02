//! Configuracao da tomada (`Tomada`) — parametros do modelo MILP, versionada.
//!
//! Espelha o jsonb `tomadas.config` do schema (001 `supabase-schema.sql`) + a coluna
//! `config_version`. Persistida localmente em LittleFS (`/lfs/config.json`) para operar
//! offline-first (Principio VI); o pull da nuvem so adota uma versao remota MAIOR
//! (`sync::deve_adotar_config`, FR-015).
//!
//! Mapeamento modelo (data-model.md §2):
//!   is_critical -> W_i = M ; w_i -> peso ; p_max -> teto R2 ; t_limite -> F_i ;
//!   schedule[24] -> A_{i,t} ; pi_historico[24] -> Pi_{i,t} ; p_limiar_detectado_auto -> U_i.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;

use crate::persistencia::littlefs;

/// Valor grande para criticidade (W_i = M quando `is_critical`).
pub const W_CRITICO: f32 = 1.0e6;

/// Parametros do modelo para esta tomada. Os campos ate `p_limiar_detectado_auto`
/// correspondem 1:1 ao jsonb `tomadas.config`; `config_version` e a coluna separada.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tomada {
    /// `is_critical`: usuario marcou "nao pode desligar" => W_i = M.
    #[serde(default)]
    pub is_critical: bool,
    /// `w_i`: peso de criticidade (>= 1; M se critico). Float p/ casar com o solver.
    #[serde(default = "um")]
    pub w_i: f32,
    /// `p_max` (W): teto global R2 (igual p/ a residencia).
    #[serde(default = "p_max_default")]
    pub p_max: f32,
    /// `t_limite` (s): limiar de ociosidade -> F_i (fantasma).
    #[serde(default = "t_limite_default")]
    pub t_limite: i64,
    /// `A_{i,t}`: agendamento por hora (true = permitido).
    #[serde(default = "schedule_default")]
    pub schedule: [bool; 24],
    /// `Pi_{i,t}`: probabilidade historica de uso por hora (0..1).
    #[serde(default = "pi_default")]
    pub pi_historico: [f32; 24],
    /// `P_limiar,i` (W) autodetectado pelo firmware (U_i = P_atual > limiar). None = nao detectado.
    #[serde(default)]
    pub p_limiar_detectado_auto: Option<f32>,
    /// Versao da config (coluna `config_version`, NAO parte do jsonb). Controle de pull.
    #[serde(default)]
    pub config_version: i64,
}

fn um() -> f32 { 1.0 }
fn p_max_default() -> f32 { 2000.0 }
fn t_limite_default() -> i64 { 600 }
fn schedule_default() -> [bool; 24] { [true; 24] }
fn pi_default() -> [f32; 24] { [0.5; 24] }

impl Default for Tomada {
    fn default() -> Self {
        Tomada {
            is_critical: false,
            w_i: um(),
            p_max: p_max_default(),
            t_limite: t_limite_default(),
            schedule: schedule_default(),
            pi_historico: pi_default(),
            p_limiar_detectado_auto: None,
            config_version: 0,
        }
    }
}

impl Tomada {
    /// Peso efetivo de criticidade para o solver (`W_i`): M se critico, senao `w_i`.
    pub fn w_efetivo(&self) -> f32 {
        if self.is_critical { W_CRITICO } else { self.w_i }
    }

    /// Monta APENAS o objeto jsonb `config` (sem `config_version`) para publicar na nuvem.
    /// JSON manual com `{:.2}` nos floats (bug LLVM Xtensa em serde de f32). Espelha a
    /// "Estrutura esperada de config" do schema 001.
    pub fn montar_config_jsonb(&self) -> String {
        let schedule_str = self
            .schedule
            .iter()
            .map(|b| if *b { "true" } else { "false" })
            .collect::<Vec<_>>()
            .join(",");
        let pi_str = self
            .pi_historico
            .iter()
            .map(|v| format!("{:.4}", v))
            .collect::<Vec<_>>()
            .join(",");
        let p_limiar_str = match self.p_limiar_detectado_auto {
            Some(v) => format!("{:.2}", v),
            None => "null".to_string(),
        };
        format!(
            r#"{{"is_critical":{},"w_i":{:.2},"p_max":{:.2},"t_limite":{},"schedule":[{}],"pi_historico":[{}],"p_limiar_detectado_auto":{}}}"#,
            self.is_critical,
            self.w_i,
            self.p_max,
            self.t_limite,
            schedule_str,
            pi_str,
            p_limiar_str,
        )
    }
}

fn caminho() -> String {
    format!("{}/config.json", littlefs::base())
}

/// Carrega a config local. Se o arquivo nao existir (1o boot), retorna `Tomada::default()`
/// e o registra — o dispositivo opera com defaults seguros ate o 1o pull/configuracao.
pub fn load() -> Result<Tomada> {
    let path = caminho();
    match std::fs::read_to_string(&path) {
        Ok(s) => {
            let t: Tomada = serde_json::from_str(&s)
                .with_context(|| format!("config.json invalido em {path}"))?;
            Ok(t)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            log::info!("[CONFIG] {path} ausente -> usando defaults");
            Ok(Tomada::default())
        }
        Err(e) => Err(e).with_context(|| format!("falha lendo {path}")),
    }
}

/// Persiste a config local em LittleFS (serde_json; compilado em opt-level=0 no workspace).
/// Escrita pontual (em pull/calibracao), NUNCA por minuto (Principio III).
pub fn save(t: &Tomada) -> Result<()> {
    let path = caminho();
    let json = serde_json::to_string(t).context("serializando config")?;
    let mut f = std::fs::File::create(&path).with_context(|| format!("criando {path}"))?;
    f.write_all(json.as_bytes())?;
    f.flush()?;
    log::info!("[CONFIG] salvo {path} (config_version={})", t.config_version);
    Ok(())
}
