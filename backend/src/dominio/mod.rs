//! Structs de domínio espelhando o schema MySQL (data-model.md).
//! `FromRow` mapeia por NOME de coluna (usado com `sqlx::query_as`).
use chrono::NaiveDateTime;
use serde::Serialize;
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Usuario {
    pub id: String,
    pub email: String,
    #[serde(skip_serializing)]
    pub senha_hash: String,
    pub criado_em: NaiveDateTime,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Residencia {
    pub id: String,
    pub owner_id: String,
    pub nome: String,
    pub p_max: i32,
    pub criado_em: NaiveDateTime,
}

// --- Entidades do plano de dados do dispositivo (usadas a partir de US2) ---

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Tomada {
    pub id: String,
    pub residencia_id: String,
    pub mac: String,
    pub nome: String,
    pub estado: String,
    /// Tri-estado (migration 0002): `None` = automático (o MILP decide),
    /// `Some(1)` = cortar, `Some(0)` = "usar agora" (janela temporária no firmware).
    pub rele_aberto: Option<i8>,
    pub cstd: f32,
    pub con: f32,
    pub p_medida_atual: f32,
    pub ultimo_uso_ativo_ts: i64,
    pub config: serde_json::Value,
    pub config_version: i64,
    pub criado_em: NaiveDateTime,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Medicao {
    pub id: i64,
    pub tomada_id: String,
    pub timestamp: i64,
    /// Potência **ATIVA** (W). Medições anteriores a 2026-08-24 gravaram aqui a
    /// potência aparente (VA) — são justamente as que têm `fator_potencia` nulo.
    pub p_medida: f32,
    /// `P/S` em [0, 1] da mesma janela, ou `None` para as medições antigas
    /// (migration 0002): a fase não foi gravada, então não há backfill possível.
    pub fator_potencia: Option<f32>,
    pub estado: String,
    pub rele_aberto: i8,
    pub criado_em: NaiveDateTime,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Evento {
    pub id: i64,
    pub tomada_id: Option<String>,
    pub timestamp: i64,
    pub tipo: String,
    pub severidade: String,
    pub titulo: String,
    pub detalhes: Option<String>,
    pub criado_em: NaiveDateTime,
}

#[derive(Debug, Clone, FromRow)]
pub struct DeviceCredential {
    pub tomada_id: String,
    pub secret_hash: String,
    pub criado_em: NaiveDateTime,
    pub revoked_at: Option<NaiveDateTime>,
    pub ultima_renovacao: Option<NaiveDateTime>,
}

#[derive(Debug, Clone, FromRow)]
pub struct ProvisioningClaim {
    pub id: String,
    pub residencia_id: String,
    pub code_hash: String,
    pub expira_em: NaiveDateTime,
    pub usado_em: Option<NaiveDateTime>,
    pub criado_em: NaiveDateTime,
}

#[derive(Debug, Clone, FromRow)]
pub struct ResidenciaSecret {
    pub residencia_id: String,
    pub residencia_key: String,
    pub criado_em: NaiveDateTime,
}
