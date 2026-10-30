//! US1 - cadastro e login de usuário (substitui o Supabase Auth).
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
    // Resposta IDENTICA para e-mail novo e para e-mail que já tem conta. O 409
    // "email já cadastrado" respondia a qualquer anônimo, sem credencial nenhuma,
    // se um endereço tem conta aqui - enumeração de usuários. O `hash_senha` acima
    // já rodou nos dois caminhos, então o tempo de resposta também não separa os
    // casos. Quem tenta cadastrar de novo cai no login seguinte e leva 401 se a
    // senha não for a da conta que já existe.
    match db::usuarios::inserir(&pool, &id, &body.email, &hash).await {
        Ok(_) => Ok(HttpResponse::Created().json(json!({ "ok": true }))),
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            log::info!("registro repetido para e-mail já cadastrado; 201 genérico");
            Ok(HttpResponse::Created().json(json!({ "ok": true })))
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
    // O status já era 401 nos dois casos, mas o relógio contava a diferença:
    // e-mail sem conta voltava em ~5 ms (nenhum Argon2 rodava) e e-mail com conta
    // em ~385 ms. Pagar o mesmo Argon2 contra um hash de referência fecha esse
    // canal - é o mesmo vazamento que o 409 do `registrar`, só que por tempo.
    let u = match db::usuarios::buscar_por_email(&pool, &body.email).await? {
        Some(u) => u,
        None => {
            senha::verificar_dummy(&body.senha);
            return Err(ApiErro::NaoAutorizado);
        }
    };
    if !senha::verificar_senha(&body.senha, &u.senha_hash) {
        return Err(ApiErro::NaoAutorizado);
    }
    let (token, exp) = jwt::emitir_usuario(&u.id, &cfg.jwt_secret, cfg.jwt_ttl_secs);
    Ok(HttpResponse::Ok().json(json!({ "jwt": token, "jwt_exp": exp })))
}
