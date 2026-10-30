use std::env;

/// Configuração do backend, carregada de variáveis de ambiente (`.env`).
/// `jwt_secret` e a credencial do banco vivem APENAS aqui (Princípio VIII).
#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub jwt_secret: String,
    pub bind_addr: String,
    pub jwt_ttl_secs: i64,
    /// Origens permitidas no CORS - só o **dashboard** precisa disso. O firmware
    /// não manda `Origin` (CORS é uma política de navegador), então esta lista não
    /// afeta o plano de dados do device.
    pub cors_origins: Vec<String>,
}

/// Origens padrão: o servidor de dev do Vite. Em LAN, adicione o host de onde o
/// `frontend/dist` for servido via `CORS_ORIGINS`.
const CORS_PADRAO: &str = "http://localhost:5173,http://127.0.0.1:5173";

impl Config {
    pub fn carregar() -> Self {
        let _ = dotenvy::dotenv();
        Config {
            database_url: env::var("DATABASE_URL").expect("DATABASE_URL não definido"),
            jwt_secret: env::var("JWT_SECRET").expect("JWT_SECRET não definido"),
            bind_addr: env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string()),
            jwt_ttl_secs: env::var("JWT_TTL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3600),
            cors_origins: env::var("CORS_ORIGINS")
                .unwrap_or_else(|_| CORS_PADRAO.to_string())
                .split(',')
                .map(|s| s.trim().trim_end_matches('/').to_string())
                .filter(|s| !s.is_empty())
                .collect(),
        }
    }
}
