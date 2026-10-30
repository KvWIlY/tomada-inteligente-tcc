//! US1 - residências (dono único, isolamento por owner). Cada handler exige
//! `UsuarioAutenticado`; sem token → 401; residência de outro dono → 403.
use crate::auth::{senha, UsuarioAutenticado};
use crate::db;
use crate::erros::ApiErro;
use actix_web::{web, HttpResponse};
use chrono::Utc;
use rand::Rng;
use serde::Deserialize;
use serde_json::json;
use sqlx::MySqlPool;
use uuid::Uuid;

/// Minutos de validade do claim-code (contrato: ~10 min).
const CLAIM_TTL_MIN: i64 = 10;
/// Alfabeto sem caracteres ambíguos (I/1, O/0) - o código é digitado à mão no
/// portal AP da tomada.
const ALFABETO_CLAIM: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

#[derive(Deserialize)]
pub struct CriarReq {
    pub nome: String,
    pub p_max: i32,
}

#[derive(Deserialize)]
pub struct PatchReq {
    pub nome: Option<String>,
    pub p_max: Option<i32>,
}

pub async fn listar(
    pool: web::Data<MySqlPool>,
    user: UsuarioAutenticado,
) -> Result<HttpResponse, ApiErro> {
    let rs = db::residencias::listar_por_owner(&pool, &user.user_id).await?;
    Ok(HttpResponse::Ok().json(rs))
}

pub async fn criar(
    pool: web::Data<MySqlPool>,
    user: UsuarioAutenticado,
    body: web::Json<CriarReq>,
) -> Result<HttpResponse, ApiErro> {
    if body.p_max <= 0 {
        return Err(ApiErro::Validacao("p_max deve ser > 0".to_string()));
    }
    if body.nome.trim().is_empty() {
        return Err(ApiErro::Validacao("nome obrigatório".to_string()));
    }
    let id = Uuid::new_v4().to_string();
    db::residencias::inserir(&pool, &id, &user.user_id, &body.nome, body.p_max).await?;
    let r = db::residencias::buscar_por_id(&pool, &id)
        .await?
        .ok_or_else(|| ApiErro::Interno("residência não encontrada após inserir".to_string()))?;
    Ok(HttpResponse::Created().json(r))
}

/// T041 [US3] - detalhe + visão geral (UC04): junta os campos da residência com
/// o consumo instantâneo e o score de eficiência das últimas 24h.
pub async fn obter(
    pool: web::Data<MySqlPool>,
    user: UsuarioAutenticado,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiErro> {
    let id = path.into_inner();
    let r = db::residencias::buscar_por_id(&pool, &id)
        .await?
        .ok_or(ApiErro::NaoEncontrado)?;
    if r.owner_id != user.user_id {
        return Err(ApiErro::Proibido);
    }
    let desde = Utc::now().timestamp() - 24 * 3600;
    let vg = db::tomadas::visao_geral(&pool, &id, desde).await?;
    Ok(HttpResponse::Ok().json(json!({
        "id": r.id,
        "owner_id": r.owner_id,
        "nome": r.nome,
        "p_max": r.p_max,
        "criado_em": r.criado_em,
        "tomadas": vg.tomadas,
        "consumo_total_atual": vg.consumo_total_atual,
        "efficiency_score": vg.efficiency_score,
    })))
}

/// Lista as tomadas da residência (UC04) - a grade do dashboard.
pub async fn listar_tomadas(
    pool: web::Data<MySqlPool>,
    user: UsuarioAutenticado,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiErro> {
    let id = path.into_inner();
    let r = db::residencias::buscar_por_id(&pool, &id)
        .await?
        .ok_or(ApiErro::NaoEncontrado)?;
    if r.owner_id != user.user_id {
        return Err(ApiErro::Proibido);
    }
    let ts = db::tomadas::listar_por_residencia(&pool, &id).await?;
    Ok(HttpResponse::Ok().json(ts))
}

pub async fn atualizar(
    pool: web::Data<MySqlPool>,
    user: UsuarioAutenticado,
    path: web::Path<String>,
    body: web::Json<PatchReq>,
) -> Result<HttpResponse, ApiErro> {
    let id = path.into_inner();
    let r = db::residencias::buscar_por_id(&pool, &id)
        .await?
        .ok_or(ApiErro::NaoEncontrado)?;
    if r.owner_id != user.user_id {
        return Err(ApiErro::Proibido);
    }
    if let Some(p) = body.p_max {
        if p <= 0 {
            return Err(ApiErro::Validacao("p_max deve ser > 0".to_string()));
        }
    }
    db::residencias::atualizar(&pool, &id, &user.user_id, body.nome.as_deref(), body.p_max).await?;
    let atual = db::residencias::buscar_por_id(&pool, &id)
        .await?
        .ok_or(ApiErro::NaoEncontrado)?;
    Ok(HttpResponse::Ok().json(atual))
}

/// T027 [US2] - gera um claim-code de pareamento (UC03). O código em claro sai
/// só nesta resposta; o banco guarda apenas `sha256(código)`.
pub async fn criar_claim_code(
    pool: web::Data<MySqlPool>,
    user: UsuarioAutenticado,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiErro> {
    let residencia_id = path.into_inner();
    let r = db::residencias::buscar_por_id(&pool, &residencia_id)
        .await?
        .ok_or(ApiErro::NaoEncontrado)?;
    if r.owner_id != user.user_id {
        return Err(ApiErro::Proibido);
    }

    let codigo: String = {
        let mut rng = rand::thread_rng();
        (0..8)
            .map(|_| ALFABETO_CLAIM[rng.gen_range(0..ALFABETO_CLAIM.len())] as char)
            .collect()
    };
    let id = Uuid::new_v4().to_string();
    let expira_em = db::credenciais::inserir_claim(
        &pool,
        &id,
        &residencia_id,
        &senha::sha256_hex(&codigo),
        CLAIM_TTL_MIN,
    )
    .await?;

    Ok(HttpResponse::Created().json(json!({
        "id": id,
        "claim_code": codigo,
        "expira_em": expira_em.and_utc().timestamp(),
    })))
}
