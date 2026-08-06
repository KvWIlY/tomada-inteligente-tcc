//! US1 — cadastro e login de usuário (substitui o Supabase Auth).
use crate::auth::limite;
use crate::auth::{jwt, senha};
use crate::config::Config;
use crate::db;
use crate::erros::ApiErro;
use actix_web::{web, HttpRequest, HttpResponse};
use serde::Deserialize;
use serde_json::json;
use sqlx::MySqlPool;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct RegistroReq {
    pub email: String,
    pub senha: String,
}

#[derive(Deserialize)]
pub struct LoginReq {
    pub email: String,
    pub senha: String,
}

pub async fn registrar(
    req: HttpRequest,
    pool: web::Data<MySqlPool>,
    body: web::Json<RegistroReq>,
) -> Result<HttpResponse, ApiErro> {
    limite::checar(&req)?;
    if body.email.trim().is_empty() || body.senha.len() < 6 {
        return Err(ApiErro::Validacao(
            "email obrigatório e senha com pelo menos 6 caracteres".to_string(),
        ));
    }
    let hash = senha::hash_senha(&body.senha).map_err(ApiErro::Interno)?;
    let id = Uuid::new_v4().to_string();
    match db::usuarios::inserir(&pool, &id, &body.email, &hash).await {
        Ok(_) => Ok(HttpResponse::Created().json(json!({ "id": id }))),
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            Err(ApiErro::Conflito("email já cadastrado".to_string()))
        }
        Err(e) => Err(ApiErro::Interno(e.to_string())),
    }
}

pub async fn login(
    req: HttpRequest,
    pool: web::Data<MySqlPool>,
    cfg: web::Data<Config>,
    body: web::Json<LoginReq>,
) -> Result<HttpResponse, ApiErro> {
    limite::checar(&req)?;
    let u = db::usuarios::buscar_por_email(&pool, &body.email)
        .await?
        .ok_or(ApiErro::NaoAutorizado)?;
    if !senha::verificar_senha(&body.senha, &u.senha_hash) {
        return Err(ApiErro::NaoAutorizado);
    }
    let (token, exp) = jwt::emitir_usuario(&u.id, &cfg.jwt_secret, cfg.jwt_ttl_secs);
    Ok(HttpResponse::Ok().json(json!({ "jwt": token, "jwt_exp": exp })))
}
