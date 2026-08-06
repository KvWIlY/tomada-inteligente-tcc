use actix_web::{http::StatusCode, HttpResponse, ResponseError};
use serde_json::json;
use std::fmt;

/// Erro de API mapeado para resposta HTTP. Default-deny: qualquer falha de
/// autenticação/autorização vira 401/403 com corpo `{ erro, msg }`.
#[derive(Debug)]
pub enum ApiErro {
    PayloadInvalido(String), // 400
    NaoAutorizado,           // 401
    Proibido,                // 403
    NaoEncontrado,           // 404
    Conflito(String),        // 409
    Validacao(String),       // 422
    MuitasTentativas,        // 429
    Interno(String),         // 500
}

impl ApiErro {
    fn codigo(&self) -> &'static str {
        match self {
            ApiErro::PayloadInvalido(_) => "payload_invalido",
            ApiErro::NaoAutorizado => "nao_autorizado",
            ApiErro::Proibido => "proibido",
            ApiErro::NaoEncontrado => "nao_encontrado",
            ApiErro::Conflito(_) => "conflito",
            ApiErro::Validacao(_) => "validacao",
            ApiErro::MuitasTentativas => "muitas_tentativas",
            ApiErro::Interno(_) => "interno",
        }
    }
}

impl fmt::Display for ApiErro {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiErro::PayloadInvalido(m)
            | ApiErro::Conflito(m)
            | ApiErro::Validacao(m)
            | ApiErro::Interno(m) => write!(f, "{m}"),
            ApiErro::NaoAutorizado => write!(f, "não autorizado"),
            ApiErro::Proibido => write!(f, "acesso negado"),
            ApiErro::NaoEncontrado => write!(f, "não encontrado"),
            ApiErro::MuitasTentativas => write!(f, "muitas tentativas; tente de novo em instantes"),
        }
    }
}

impl ResponseError for ApiErro {
    fn status_code(&self) -> StatusCode {
        match self {
            ApiErro::PayloadInvalido(_) => StatusCode::BAD_REQUEST,
            ApiErro::NaoAutorizado => StatusCode::UNAUTHORIZED,
            ApiErro::Proibido => StatusCode::FORBIDDEN,
            ApiErro::NaoEncontrado => StatusCode::NOT_FOUND,
            ApiErro::Conflito(_) => StatusCode::CONFLICT,
            ApiErro::Validacao(_) => StatusCode::UNPROCESSABLE_ENTITY,
            ApiErro::MuitasTentativas => StatusCode::TOO_MANY_REQUESTS,
            ApiErro::Interno(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code())
            .json(json!({ "erro": self.codigo(), "msg": self.to_string() }))
    }
}

impl From<sqlx::Error> for ApiErro {
    fn from(e: sqlx::Error) -> Self {
        ApiErro::Interno(e.to_string())
    }
}
