//! Emissão/validação de JWT HS256 para usuários (dashboard) e dispositivos
//! (firmware). O segredo de assinatura vem da Config e vive só no backend
//! (Princípio VIII).
use chrono::Utc;
use jsonwebtoken::{
    decode, encode, errors::Error, Algorithm, DecodingKey, EncodingKey, Header, Validation,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub typ: String, // "user" | "device"
    pub sub: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tomada_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub residencia_id: Option<String>,
    pub iat: i64,
    pub exp: i64,
}

fn assinar(claims: &Claims, secret: &str) -> String {
    encode(
        &Header::new(Algorithm::HS256),
        claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .expect("falha ao assinar JWT")
}

pub fn emitir_usuario(user_id: &str, secret: &str, ttl: i64) -> (String, i64) {
    let iat = Utc::now().timestamp();
    let exp = iat + ttl;
    let c = Claims {
        typ: "user".to_string(),
        sub: user_id.to_string(),
        tomada_id: None,
        residencia_id: None,
        iat,
        exp,
    };
    (assinar(&c, secret), exp)
}

pub fn emitir_device(tomada_id: &str, residencia_id: &str, secret: &str, ttl: i64) -> (String, i64) {
    let iat = Utc::now().timestamp();
    let exp = iat + ttl;
    let c = Claims {
        typ: "device".to_string(),
        sub: tomada_id.to_string(),
        tomada_id: Some(tomada_id.to_string()),
        residencia_id: Some(residencia_id.to_string()),
        iat,
        exp,
    };
    (assinar(&c, secret), exp)
}

pub fn validar(token: &str, secret: &str) -> Result<Claims, Error> {
    let mut v = Validation::new(Algorithm::HS256);
    v.validate_exp = true;
    let dados = decode::<Claims>(token, &DecodingKey::from_secret(secret.as_bytes()), &v)?;
    Ok(dados.claims)
}
