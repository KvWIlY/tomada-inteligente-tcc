use crate::dominio::Usuario;
use chrono::Utc;
use sqlx::MySqlPool;

pub async fn inserir(
    pool: &MySqlPool,
    id: &str,
    email: &str,
    senha_hash: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO usuarios (id, email, senha_hash, criado_em) VALUES (?, ?, ?, ?)")
        .bind(id)
        .bind(email)
        .bind(senha_hash)
        .bind(Utc::now().naive_utc())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn buscar_por_email(
    pool: &MySqlPool,
    email: &str,
) -> Result<Option<Usuario>, sqlx::Error> {
    sqlx::query_as::<_, Usuario>(
        "SELECT id, email, senha_hash, criado_em FROM usuarios WHERE email = ?",
    )
    .bind(email)
    .fetch_optional(pool)
    .await
}
