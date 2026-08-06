use actix_cors::Cors;
use actix_web::http::header;
use actix_web::middleware::Logger;
use actix_web::{web, App, HttpServer};
use backend::{config::Config, db, rotas_config};

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // T054 — observabilidade (Princípio VII). `RUST_LOG` controla o nível;
    // `info` por padrão para que decisões de autorização e falhas apareçam sem
    // precisar reconfigurar nada em campo.
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    let cfg = Config::carregar();
    let pool = db::criar_pool(&cfg.database_url)
        .await
        .expect("falha ao conectar no MySQL (confira DATABASE_URL)");
    db::rodar_migrations(&pool)
        .await
        .expect("falha ao aplicar migrations");
    log::info!("migrations aplicadas");

    let bind = cfg.bind_addr.clone();
    let origens = cfg.cors_origins.clone();
    log::info!("CORS liberado para: {}", origens.join(", "));
    let pool_data = web::Data::new(pool);
    let cfg_data = web::Data::new(cfg);
    log::info!("backend ouvindo em {bind}");

    HttpServer::new(move || {
        // O dashboard roda noutra origem (Vite em :5173, ou o `dist/` servido de
        // outro host na LAN), então o navegador exige CORS — inclusive um
        // preflight OPTIONS antes de todo POST/PATCH/DELETE com JSON. Lista
        // explícita em vez de `permissive()`: default-deny também aqui
        // (Princípio VIII), e não usamos cookies — o JWT vai no header
        // Authorization, então `supports_credentials` é desnecessário.
        let mut cors = Cors::default()
            .allowed_methods(vec!["GET", "POST", "PATCH", "DELETE", "OPTIONS"])
            .allowed_headers(vec![header::AUTHORIZATION, header::CONTENT_TYPE])
            .max_age(3600);
        for o in &origens {
            cors = cors.allowed_origin(o);
        }

        App::new()
            .app_data(pool_data.clone())
            .app_data(cfg_data.clone())
            .wrap(cors)
            // Uma linha por requisição: método, rota, status e duração. NUNCA
            // registra corpo nem `Authorization` — segredos não vão para o log
            // (Princípio VII/VIII).
            .wrap(Logger::new("%r -> %s (%Dms) de %a"))
            .configure(rotas_config)
    })
    .bind(&bind)?
    .run()
    .await
}
