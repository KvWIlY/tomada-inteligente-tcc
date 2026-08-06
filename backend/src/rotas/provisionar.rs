//! T028 [US2] — `POST /provisionar` e `POST /renovar`.
//!
//! São os únicos endpoints públicos além de `/auth/*`: o device ainda não tem
//! JWT quando chega aqui. A credencial de entrada é o **claim-code** (uso único,
//! curta expiração) no `/provisionar` e o **device_secret** durável no
//! `/renovar`. O MAC identifica, não autentica.
//! Contrato: contracts/provisioning-api.md
use crate::auth::limite;
use crate::auth::{jwt, senha};
use crate::config::Config;
use crate::db;
use crate::erros::ApiErro;
use actix_web::{web, HttpRequest, HttpResponse};
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;
use sqlx::MySqlPool;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct ProvisionarReq {
    pub mac: String,
    pub claim_code: String,
}

#[derive(Deserialize)]
pub struct RenovarReq {
    pub tomada_id: String,
    pub device_secret: String,
}

/// `AA:BB:CC:DD:EE:FF` — 6 octetos hex separados por `:`. Normaliza para
/// maiúsculas, que é como o firmware formata `esp_efuse_mac_get_default()`.
fn normalizar_mac(bruto: &str) -> Result<String, ApiErro> {
    let mac = bruto.trim().to_uppercase();
    let octetos: Vec<&str> = mac.split(':').collect();
    let valido = octetos.len() == 6
        && octetos
            .iter()
            .all(|o| o.len() == 2 && o.chars().all(|c| c.is_ascii_hexdigit()));
    if !valido {
        return Err(ApiErro::PayloadInvalido(
            "mac deve estar no formato AA:BB:CC:DD:EE:FF".to_string(),
        ));
    }
    Ok(mac)
}

pub async fn provisionar(
    req: HttpRequest,
    pool: web::Data<MySqlPool>,
    cfg: web::Data<Config>,
    body: web::Json<ProvisionarReq>,
) -> Result<HttpResponse, ApiErro> {
    limite::checar(&req)?;
    let mac = normalizar_mac(&body.mac)?;
    if body.claim_code.trim().is_empty() {
        return Err(ApiErro::PayloadInvalido("claim_code vazio".to_string()));
    }

    // Tudo numa transação: ou o device sai daqui com identidade + credencial +
    // claim consumido, ou nada aconteceu.
    let mut tx = pool.begin().await?;

    let claim = db::credenciais::buscar_claim_por_hash(&mut tx, &senha::sha256_hex(&body.claim_code))
        .await?
        .ok_or(ApiErro::NaoAutorizado)?;
    if claim.usado_em.is_some() || claim.expira_em <= Utc::now().naive_utc() {
        return Err(ApiErro::NaoAutorizado);
    }
    // Consumo atômico — quem perde a corrida recebe 401, não uma 2ª identidade.
    if !db::credenciais::consumir_claim(&mut tx, &claim.id).await? {
        return Err(ApiErro::NaoAutorizado);
    }

    // Identidade por MAC: re-provisionar o mesmo chip reaproveita a tomada.
    let tomada_id = match db::tomadas::buscar_por_mac(&mut tx, &mac).await? {
        Some(t) if t.residencia_id != claim.residencia_id => {
            return Err(ApiErro::Conflito(
                "MAC já registrado em outra residência".to_string(),
            ));
        }
        Some(t) => t.id,
        None => {
            let id = Uuid::new_v4().to_string();
            let nome = format!("Tomada {}", &mac[12..]);
            db::tomadas::inserir(&mut tx, &id, &claim.residencia_id, &mac, &nome).await?;
            id
        }
    };

    // Segredo durável: texto devolvido uma única vez, só o hash fica no banco.
    let device_secret = senha::gerar_segredo();
    db::credenciais::upsert_credencial(&mut tx, &tomada_id, &senha::sha256_hex(&device_secret))
        .await?;

    let residencia_key = db::credenciais::obter_ou_criar_residencia_key(
        &mut tx,
        &claim.residencia_id,
        senha::gerar_segredo,
    )
    .await?;

    tx.commit().await?;

    let (token, exp) = jwt::emitir_device(
        &tomada_id,
        &claim.residencia_id,
        &cfg.jwt_secret,
        cfg.jwt_ttl_secs,
    );
    Ok(HttpResponse::Ok().json(json!({
        "tomada_id": tomada_id,
        "residencia_id": claim.residencia_id,
        "device_secret": device_secret,
        "residencia_key": residencia_key,
        "jwt": token,
        "jwt_exp": exp,
    })))
}

pub async fn renovar(
    req: HttpRequest,
    pool: web::Data<MySqlPool>,
    cfg: web::Data<Config>,
    body: web::Json<RenovarReq>,
) -> Result<HttpResponse, ApiErro> {
    limite::checar(&req)?;
    let cred = db::credenciais::buscar_credencial(&pool, &body.tomada_id)
        .await?
        .ok_or(ApiErro::NaoEncontrado)?;
    if cred.revoked_at.is_some() {
        return Err(ApiErro::NaoAutorizado);
    }
    if !senha::comparar_ct(&senha::sha256_hex(&body.device_secret), &cred.secret_hash) {
        return Err(ApiErro::NaoAutorizado);
    }

    // A residência vem do banco, nunca do corpo — o device não escolhe escopo.
    let tomada = db::tomadas::buscar_por_id(&pool, &body.tomada_id)
        .await?
        .ok_or(ApiErro::NaoEncontrado)?;

    db::credenciais::marcar_renovacao(&pool, &body.tomada_id).await?;
    let (token, exp) = jwt::emitir_device(
        &tomada.id,
        &tomada.residencia_id,
        &cfg.jwt_secret,
        cfg.jwt_ttl_secs,
    );
    Ok(HttpResponse::Ok().json(json!({ "jwt": token, "jwt_exp": exp })))
}
