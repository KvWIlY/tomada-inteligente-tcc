//! T026 [US2] - inserção idempotente de `medicoes` e `eventos`.
//!
//! Idempotência (FR-009): o device re-envia o lote inteiro enquanto não receber
//! confirmação, então o mesmo `(tomada_id, timestamp)` chega várias vezes.
//! `medicoes` tem UNIQUE nessa dupla e usa `ON DUPLICATE KEY UPDATE`. `eventos`
//! não tem chave natural no schema, então a dedup é por
//! `(tomada_id, timestamp, tipo)` via `INSERT ... SELECT ... WHERE NOT EXISTS`.
use crate::dominio::{Evento, Medicao};
use crate::erros::ApiErro;
use chrono::Utc;
use serde::Deserialize;
use sqlx::{MySqlConnection, MySqlPool};

const ESTADOS: [&str; 3] = ["OFF", "STD", "ON"];
const SEVERIDADES: [&str; 3] = ["info", "warn", "erro"];

#[derive(Debug, Deserialize)]
pub struct MedicaoIn {
    pub timestamp: i64,
    /// Potência **ATIVA** (W): o firmware calcula `media(v_k * i_k)` sobre a janela,
    /// não `Vrms * Irms`. Medições gravadas antes de 2026-08-24 são aparentes (VA).
    pub p_medida: f32,
    /// `P/S` em [0, 1]. `default` (None) aceita firmware anterior a 2026-08-24, que
    /// não envia o campo - recusar o lote por ausência dele deixaria um nó antigo
    /// preso, reenviando para sempre um buffer que nunca é confirmado.
    #[serde(default)]
    pub fator_potencia: Option<f32>,
    pub estado: String,
    pub rele_aberto: bool,
}

#[derive(Debug, Deserialize)]
pub struct EventoIn {
    pub timestamp: i64,
    pub tipo: String,
    #[serde(default = "severidade_padrao")]
    pub severidade: String,
    pub titulo: String,
    pub detalhes: Option<String>,
}

fn severidade_padrao() -> String {
    "info".to_string()
}

/// Valida o lote ANTES de tocar o banco: um item ruim invalida o lote inteiro
/// (422), em vez de gravar metade e deixar o device sem saber o que confirmar.
pub fn validar(medicoes: &[MedicaoIn], eventos: &[EventoIn]) -> Result<(), ApiErro> {
    for m in medicoes {
        if !ESTADOS.contains(&m.estado.as_str()) {
            return Err(ApiErro::Validacao(format!(
                "estado inválido: {} (esperado OFF|STD|ON)",
                m.estado
            )));
        }
        if !m.p_medida.is_finite() || m.p_medida < 0.0 {
            return Err(ApiErro::Validacao(
                "p_medida deve ser um número >= 0".to_string(),
            ));
        }
        if m.timestamp <= 0 {
            return Err(ApiErro::Validacao(
                "timestamp deve ser epoch positivo".to_string(),
            ));
        }
        if let Some(fp) = m.fator_potencia {
            if !fp.is_finite() || !(0.0..=1.0).contains(&fp) {
                return Err(ApiErro::Validacao(
                    "fator_potencia deve estar entre 0 e 1".to_string(),
                ));
            }
        }
    }
    for e in eventos {
        if !SEVERIDADES.contains(&e.severidade.as_str()) {
            return Err(ApiErro::Validacao(format!(
                "severidade inválida: {} (esperado info|warn|erro)",
                e.severidade
            )));
        }
        if e.titulo.trim().is_empty() {
            return Err(ApiErro::Validacao("titulo obrigatório".to_string()));
        }
    }
    Ok(())
}

pub async fn inserir_medicoes(
    conn: &mut MySqlConnection,
    tomada_id: &str,
    medicoes: &[MedicaoIn],
) -> Result<(), sqlx::Error> {
    for m in medicoes {
        sqlx::query(
            "INSERT INTO medicoes (tomada_id, timestamp, p_medida, fator_potencia, estado, rele_aberto, criado_em) \
             VALUES (?, ?, ?, ?, ?, ?, ?) \
             ON DUPLICATE KEY UPDATE p_medida = VALUES(p_medida), fator_potencia = VALUES(fator_potencia), estado = VALUES(estado), \
             rele_aberto = VALUES(rele_aberto)",
        )
        .bind(tomada_id)
        .bind(m.timestamp)
        .bind(m.p_medida)
        .bind(m.fator_potencia)
        .bind(&m.estado)
        .bind(m.rele_aberto as i8)
        .bind(Utc::now().naive_utc())
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

pub async fn inserir_eventos(
    conn: &mut MySqlConnection,
    tomada_id: &str,
    eventos: &[EventoIn],
) -> Result<(), sqlx::Error> {
    for e in eventos {
        sqlx::query(
            "INSERT INTO eventos (tomada_id, timestamp, tipo, severidade, titulo, detalhes, criado_em) \
             SELECT ?, ?, ?, ?, ?, ?, ? FROM DUAL WHERE NOT EXISTS ( \
               SELECT 1 FROM eventos WHERE tomada_id = ? AND timestamp = ? AND tipo = ? \
             )",
        )
        .bind(tomada_id)
        .bind(e.timestamp)
        .bind(&e.tipo)
        .bind(&e.severidade)
        .bind(&e.titulo)
        .bind(&e.detalhes)
        .bind(Utc::now().naive_utc())
        .bind(tomada_id)
        .bind(e.timestamp)
        .bind(&e.tipo)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

// ── Leitura (US3 - dashboard) ───────────────────────────────────────────────

/// Teto de pontos por série: 7 dias amostrados a cada minuto dão ~10k linhas;
/// o corte evita que uma tomada muito falante derrube o gráfico.
const LIMITE_SERIE: i64 = 20_000;

pub async fn listar_medicoes(
    pool: &MySqlPool,
    tomada_id: &str,
    desde_ts: i64,
) -> Result<Vec<Medicao>, sqlx::Error> {
    sqlx::query_as::<_, Medicao>(
        "SELECT id, tomada_id, timestamp, p_medida, fator_potencia, estado, rele_aberto, criado_em \
         FROM medicoes WHERE tomada_id = ? AND timestamp >= ? \
         ORDER BY timestamp LIMIT ?",
    )
    .bind(tomada_id)
    .bind(desde_ts)
    .bind(LIMITE_SERIE)
    .fetch_all(pool)
    .await
}

pub async fn listar_eventos(
    pool: &MySqlPool,
    tomada_id: &str,
    limite: i64,
) -> Result<Vec<Evento>, sqlx::Error> {
    sqlx::query_as::<_, Evento>(
        "SELECT id, tomada_id, timestamp, tipo, severidade, titulo, detalhes, criado_em \
         FROM eventos WHERE tomada_id = ? ORDER BY timestamp DESC LIMIT ?",
    )
    .bind(tomada_id)
    .bind(limite)
    .fetch_all(pool)
    .await
}
