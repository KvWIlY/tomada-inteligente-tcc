//! T029/T030 [US2] — plano de dados do dispositivo: `POST /telemetria` (lote
//! idempotente) e `GET /config` (pull do desired-state).
//!
//! T031 — **o escopo vem sempre do claim** (`DispositivoAutenticado`), nunca do
//! corpo ou da URL. Um device não consegue nomear outra tomada nem que tente:
//! não há parâmetro para isso. O claim ainda é confrontado com o banco (uma
//! assinatura válida apontando para tomada inexistente → 403).
use crate::auth::DispositivoAutenticado;
use crate::db;
use crate::db::telemetria::{EventoIn, MedicaoIn};
use crate::erros::ApiErro;
use actix_web::{web, HttpResponse};
use serde::Deserialize;
use serde_json::json;
use sqlx::MySqlPool;

#[derive(Deserialize)]
pub struct TelemetriaReq {
    #[serde(default)]
    pub medicoes: Vec<MedicaoIn>,
    #[serde(default)]
    pub eventos: Vec<EventoIn>,
    /// Estado instantâneo do nó. `ultimo_uso_ativo_ts` é decidido pelo firmware
    /// (só avança quando ele detecta uso real), então o backend não o infere —
    /// apenas persiste, para que as irmãs possam lê-lo no fallback (US4/UC22).
    #[serde(default)]
    pub snapshot: Option<SnapshotReq>,
}

#[derive(Deserialize)]
pub struct SnapshotReq {
    pub p_medida_atual: f32,
    pub ultimo_uso_ativo_ts: i64,
    pub estado: String,
    pub cstd: Option<f32>,
    pub con: Option<f32>,
}

pub async fn telemetria(
    pool: web::Data<MySqlPool>,
    dev: DispositivoAutenticado,
    body: web::Json<TelemetriaReq>,
) -> Result<HttpResponse, ApiErro> {
    let tomada = db::tomadas::buscar_por_id(&pool, &dev.tomada_id)
        .await?
        .ok_or(ApiErro::Proibido)?;
    if tomada.residencia_id != dev.residencia_id {
        return Err(ApiErro::Proibido);
    }

    db::telemetria::validar(&body.medicoes, &body.eventos)?;
    if let Some(s) = &body.snapshot {
        if !["OFF", "STD", "ON"].contains(&s.estado.as_str()) {
            return Err(ApiErro::Validacao(format!(
                "estado inválido no snapshot: {}",
                s.estado
            )));
        }
        if !s.p_medida_atual.is_finite() || s.p_medida_atual < 0.0 {
            return Err(ApiErro::Validacao(
                "p_medida_atual deve ser um número >= 0".to_string(),
            ));
        }
    }

    let mut tx = pool.begin().await?;
    db::telemetria::inserir_medicoes(&mut tx, &tomada.id, &body.medicoes).await?;
    db::telemetria::inserir_eventos(&mut tx, &tomada.id, &body.eventos).await?;
    if let Some(s) = &body.snapshot {
        db::tomadas::atualizar_snapshot(
            &mut tx,
            &tomada.id,
            &db::tomadas::Snapshot {
                p_medida_atual: s.p_medida_atual,
                ultimo_uso_ativo_ts: s.ultimo_uso_ativo_ts,
                estado: s.estado.clone(),
                cstd: s.cstd,
                con: s.con,
            },
        )
        .await?;
    }
    tx.commit().await?;

    // O device libera o buffer até aqui (FR-011/UC18). `null` = nada a purgar.
    let confirmado_ate_ts = body.medicoes.iter().map(|m| m.timestamp).max();
    Ok(HttpResponse::Ok().json(json!({ "confirmado_ate_ts": confirmado_ate_ts })))
}

pub async fn config(
    pool: web::Data<MySqlPool>,
    dev: DispositivoAutenticado,
) -> Result<HttpResponse, ApiErro> {
    let tomada = db::tomadas::buscar_por_id(&pool, &dev.tomada_id)
        .await?
        .ok_or(ApiErro::Proibido)?;
    if tomada.residencia_id != dev.residencia_id {
        return Err(ApiErro::Proibido);
    }
    // Revogação verificada A CADA requisição (005/FR-013). Um JWT ainda dentro do
    // prazo NÃO basta: sem esta checagem, o dispositivo revogado continuaria puxando
    // config — e, pior, receberia a chave da casa recém-rotacionada, anulando a
    // revogação na malha ESP-NOW.
    let cred = db::credenciais::buscar_credencial(&pool, &dev.tomada_id)
        .await?
        .ok_or(ApiErro::NaoAutorizado)?;
    if cred.revoked_at.is_some() {
        return Err(ApiErro::NaoAutorizado);
    }

    let residencia = db::residencias::buscar_por_id(&pool, &tomada.residencia_id)
        .await?
        .ok_or(ApiErro::Proibido)?;
    // Chave de coordenação corrente: é assim que uma tomada legítima reingressa na
    // malha depois de uma rotação, sem re-provisionar (FR-012).
    let residencia_key = db::credenciais::buscar_residencia_key(&pool, &tomada.residencia_id)
        .await?;

    Ok(HttpResponse::Ok().json(json!({
        "config_version": tomada.config_version,
        "config": tomada.config,
        "p_max": residencia.p_max,
        "rele_aberto": tomada.rele_aberto != 0,
        "residencia_key": residencia_key,
    })))
}

/// T048 [US4] — carga das irmãs para o fallback de partição da malha (UC22).
/// O backend **não decide estados** (FR-016): devolve `p_medida_atual` e
/// `ultimo_uso_ativo_ts` crus e o firmware aplica a janela de frescor e soma
/// como `carga_fixa` — a lógica de 003 (`tomada-solver::coordenacao`) fica
/// intacta. A residência da URL precisa bater com a do claim.
pub async fn consumo_irmas(
    pool: web::Data<MySqlPool>,
    dev: DispositivoAutenticado,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiErro> {
    let residencia_id = path.into_inner();
    if residencia_id != dev.residencia_id {
        return Err(ApiErro::Proibido);
    }
    let irmas = db::tomadas::consumo_irmas(&pool, &residencia_id, &dev.tomada_id).await?;
    Ok(HttpResponse::Ok().json(json!({ "tomadas": irmas })))
}
