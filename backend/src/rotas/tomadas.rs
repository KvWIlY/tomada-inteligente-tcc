//! T038–T040 [US3] — rotas de tomada usadas pelo dashboard. Todas exigem JWT de
//! **usuário** e checam a cadeia `tomada → residência → owner_id`: um JWT de
//! device aqui é 401 (typ errado) e um dono alheio é 403 (default-deny).
use crate::auth::UsuarioAutenticado;
use crate::db;
use crate::dominio::Tomada;
use crate::erros::ApiErro;
use actix_web::{web, HttpResponse};
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;
use sqlx::MySqlPool;

/// Últimos eventos devolvidos em `/eventos` — o feed do dashboard é recente.
const LIMITE_EVENTOS: i64 = 200;

#[derive(Deserialize)]
pub struct PatchReq {
    pub nome: Option<String>,
    pub config: Option<serde_json::Value>,
    pub rele_aberto: Option<bool>,
}

#[derive(Deserialize)]
pub struct JanelaQuery {
    pub janela: Option<String>,
}

/// Porta única de autorização das rotas de tomada: resolve a tomada e confirma
/// que a residência dela é do usuário do token.
pub async fn autorizar_dono(
    pool: &MySqlPool,
    tomada_id: &str,
    user_id: &str,
) -> Result<Tomada, ApiErro> {
    let tomada = db::tomadas::buscar_por_id(pool, tomada_id)
        .await?
        .ok_or(ApiErro::NaoEncontrado)?;
    let residencia = db::residencias::buscar_por_id(pool, &tomada.residencia_id)
        .await?
        .ok_or(ApiErro::NaoEncontrado)?;
    if residencia.owner_id != user_id {
        return Err(ApiErro::Proibido);
    }
    Ok(tomada)
}

pub async fn obter(
    pool: web::Data<MySqlPool>,
    user: UsuarioAutenticado,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiErro> {
    let t = autorizar_dono(&pool, &path.into_inner(), &user.user_id).await?;
    Ok(HttpResponse::Ok().json(t))
}

pub async fn medicoes(
    pool: web::Data<MySqlPool>,
    user: UsuarioAutenticado,
    path: web::Path<String>,
    q: web::Query<JanelaQuery>,
) -> Result<HttpResponse, ApiErro> {
    let t = autorizar_dono(&pool, &path.into_inner(), &user.user_id).await?;
    let segundos = match q.janela.as_deref() {
        None | Some("24h") => 24 * 3600,
        Some("7d") => 7 * 24 * 3600,
        Some(outra) => {
            return Err(ApiErro::Validacao(format!(
                "janela inválida: {outra} (esperado 24h|7d)"
            )))
        }
    };
    let desde = Utc::now().timestamp() - segundos;
    let ms = db::telemetria::listar_medicoes(&pool, &t.id, desde).await?;
    Ok(HttpResponse::Ok().json(ms))
}

pub async fn eventos(
    pool: web::Data<MySqlPool>,
    user: UsuarioAutenticado,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiErro> {
    let t = autorizar_dono(&pool, &path.into_inner(), &user.user_id).await?;
    let evs = db::telemetria::listar_eventos(&pool, &t.id, LIMITE_EVENTOS).await?;
    Ok(HttpResponse::Ok().json(evs))
}

/// T039 — rename (UC08), config (UC06) e relé manual (UC09). Escrever `config`
/// bumpa `config_version` server-side; o nó só reaplica se a versão subiu (UC28).
pub async fn atualizar(
    pool: web::Data<MySqlPool>,
    user: UsuarioAutenticado,
    path: web::Path<String>,
    body: web::Json<PatchReq>,
) -> Result<HttpResponse, ApiErro> {
    let id = path.into_inner();
    autorizar_dono(&pool, &id, &user.user_id).await?;

    if let Some(n) = &body.nome {
        if n.trim().is_empty() {
            return Err(ApiErro::Validacao("nome não pode ser vazio".to_string()));
        }
    }
    if let Some(c) = &body.config {
        if !c.is_object() {
            return Err(ApiErro::Validacao("config deve ser um objeto".to_string()));
        }
    }
    let config = body.config.as_ref().map(|c| c.to_string());
    db::tomadas::atualizar(
        &pool,
        &id,
        body.nome.as_deref(),
        config.as_deref(),
        body.rele_aberto,
    )
    .await?;

    let atual = db::tomadas::buscar_por_id(&pool, &id)
        .await?
        .ok_or(ApiErro::NaoEncontrado)?;
    Ok(HttpResponse::Ok().json(atual))
}

/// T040 — revoga o dispositivo (UC26): `/renovar` passa a falhar, o `GET /config`
/// recusa o JWT vigente (checagem por requisição) e a **chave da residência é
/// rotacionada** (005/FR-011), tirando o hardware revogado também da coordenação
/// local. A tomada e o histórico permanecem — o que se desassocia é o hardware.
/// Re-provisionar o mesmo MAC religa.
///
/// Idempotente: revogar uma tomada já revogada NÃO gera chave nova (dois cliques no
/// dashboard não podem espalhar churn de chave pela casa).
pub async fn revogar(
    pool: web::Data<MySqlPool>,
    user: UsuarioAutenticado,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiErro> {
    let id = path.into_inner();
    autorizar_dono(&pool, &id, &user.user_id).await?;
    let tomada = db::tomadas::buscar_por_id(&pool, &id)
        .await?
        .ok_or(ApiErro::NaoEncontrado)?;

    let mut tx = pool.begin().await?;
    let revogadas = db::credenciais::revogar(&mut tx, &id).await?;
    let mut chave_rotacionada = false;
    if revogadas > 0 {
        db::credenciais::rotacionar_residencia_key(
            &mut tx,
            &tomada.residencia_id,
            crate::auth::senha::gerar_segredo,
        )
        .await?;
        chave_rotacionada = true;
    }
    tx.commit().await?;

    Ok(HttpResponse::Ok().json(json!({
        "revogado": revogadas > 0,
        "chave_rotacionada": chave_rotacionada,
    })))
}
