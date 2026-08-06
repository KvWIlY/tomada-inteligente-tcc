pub mod credenciais;
pub mod residencias;
pub mod telemetria;
pub mod tomadas;
pub mod usuarios;

use sqlx::mysql::MySqlPoolOptions;
use sqlx::MySqlPool;

pub async fn criar_pool(url: &str) -> Result<MySqlPool, sqlx::Error> {
    MySqlPoolOptions::new()
        .max_connections(10)
        .connect(url)
        .await
}

pub async fn rodar_migrations(pool: &MySqlPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await
}
