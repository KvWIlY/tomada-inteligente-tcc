//! Extractors de autenticação (default-deny). Um handler que exige autenticação
//! recebe `UsuarioAutenticado` ou `DispositivoAutenticado`; sem Bearer válido a
//! extração falha com 401 antes do handler rodar.
use crate::auth::jwt;
use crate::config::Config;
use crate::erros::ApiErro;
use actix_web::{dev::Payload, web, FromRequest, HttpRequest};
use std::future::{ready, Ready};

pub struct UsuarioAutenticado {
    pub user_id: String,
}

pub struct DispositivoAutenticado {
    pub tomada_id: String,
    pub residencia_id: String,
}

fn bearer(req: &HttpRequest) -> Option<String> {
    req.headers()
        .get("Authorization")?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(|s| s.to_string())
}

fn claims(req: &HttpRequest) -> Result<jwt::Claims, ApiErro> {
    let cfg = req
        .app_data::<web::Data<Config>>()
        .ok_or_else(|| ApiErro::Interno("config ausente no app_data".to_string()))?;
    let token = bearer(req).ok_or(ApiErro::NaoAutorizado)?;
    jwt::validar(&token, &cfg.jwt_secret).map_err(|_| ApiErro::NaoAutorizado)
}

impl FromRequest for UsuarioAutenticado {
    type Error = ApiErro;
    type Future = Ready<Result<Self, ApiErro>>;
    fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
        ready((|| {
            let c = claims(req)?;
            if c.typ != "user" {
                return Err(ApiErro::NaoAutorizado);
            }
            Ok(UsuarioAutenticado { user_id: c.sub })
        })())
    }
}

impl FromRequest for DispositivoAutenticado {
    type Error = ApiErro;
    type Future = Ready<Result<Self, ApiErro>>;
    fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
        ready((|| {
            let c = claims(req)?;
            if c.typ != "device" {
                return Err(ApiErro::NaoAutorizado);
            }
            let residencia_id = c.residencia_id.ok_or(ApiErro::NaoAutorizado)?;
            let tomada_id = c.tomada_id.ok_or(ApiErro::NaoAutorizado)?;
            Ok(DispositivoAutenticado {
                tomada_id,
                residencia_id,
            })
        })())
    }
}
