pub mod auth;
pub mod config;
pub mod db;
pub mod dominio;
pub mod erros;
pub mod rotas;

use actix_web::web;

/// Registra todas as rotas HTTP, num lugar só. É o mapa da API: quem quiser saber
/// o que o backend expõe lê esta função, não os módulos de `rotas/`.
pub fn rotas_config(cfg: &mut web::ServiceConfig) {
    cfg
        // Autenticação de usuário (US1)
        .route("/auth/register", web::post().to(rotas::auth::registrar))
        .route("/auth/login", web::post().to(rotas::auth::login))
        // Residências - JWT de usuário, escopo por dono (US1)
        .route("/residencias", web::get().to(rotas::residencias::listar))
        .route("/residencias", web::post().to(rotas::residencias::criar))
        .route("/residencias/{id}", web::get().to(rotas::residencias::obter))
        .route("/residencias/{id}", web::patch().to(rotas::residencias::atualizar))
        .route(
            "/residencias/{id}/claim-codes",
            web::post().to(rotas::residencias::criar_claim_code),
        )
        .route(
            "/residencias/{id}/tomadas",
            web::get().to(rotas::residencias::listar_tomadas),
        )
        // Tomadas - JWT de usuário, escopo por dono (US3)
        .route("/tomadas/{id}", web::patch().to(rotas::tomadas::atualizar))
        .route("/tomadas/{id}", web::delete().to(rotas::tomadas::revogar))
        .route(
            "/tomadas/{id}/medicoes",
            web::get().to(rotas::tomadas::medicoes),
        )
        .route(
            "/tomadas/{id}/eventos",
            web::get().to(rotas::tomadas::eventos),
        )
        // Provisionamento - públicos: o device ainda não tem JWT (US2)
        .route("/provisionar", web::post().to(rotas::provisionar::provisionar))
        .route("/renovar", web::post().to(rotas::provisionar::renovar))
        // Plano de dados do device - JWT de device, escopo pelo claim (US2)
        .route("/telemetria", web::post().to(rotas::sync::telemetria))
        .route("/config", web::get().to(rotas::sync::config))
        .route(
            "/residencias/{id}/tomadas/consumo",
            web::get().to(rotas::sync::consumo_irmas),
        );
}
