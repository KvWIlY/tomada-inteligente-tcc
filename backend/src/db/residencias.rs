use crate::dominio::Residencia;
use chrono::Utc;
use sqlx::MySqlPool;

const COLS: &str = "id, owner_id, nome, p_max, criado_em";

pub async fn inserir(
    pool: &MySqlPool,
    id: &str,
    owner_id: &str,
    nome: &str,
    p_max: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO residencias (id, owner_id, nome, p_max, criado_em) VALUES (?, ?, ?, ?, ?)")
        .bind(id)
        .bind(owner_id)
        .bind(nome)
        .bind(p_max)
        .bind(Utc::now().naive_utc())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn listar_por_owner(
    pool: &MySqlPool,
    owner_id: &str,
) -> Result<Vec<Residencia>, sqlx::Error> {
    sqlx::query_as::<_, Residencia>(&format!(
        "SELECT {COLS} FROM residencias WHERE owner_id = ? ORDER BY criado_em"
    ))
    .bind(owner_id)
    .fetch_all(pool)
    .await
}

pub async fn buscar_por_id(
    pool: &MySqlPool,
    id: &str,
) -> Result<Option<Residencia>, sqlx::Error> {
    sqlx::query_as::<_, Residencia>(&format!("SELECT {COLS} FROM residencias WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// Atualiza nome/p_max apenas se pertencer ao dono. Campos `None` mantêm o valor
/// atual (COALESCE). Retorna linhas afetadas (0 = não é dono / não existe).
pub async fn atualizar(
    pool: &MySqlPool,
    id: &str,
    owner_id: &str,
    nome: Option<&str>,
    p_max: Option<i32>,
) -> Result<u64, sqlx::Error> {
    let r = sqlx::query(
        "UPDATE residencias SET nome = COALESCE(?, nome), p_max = COALESCE(?, p_max) \
         WHERE id = ? AND owner_id = ?",
    )
    .bind(nome)
    .bind(p_max)
    .bind(id)
    .bind(owner_id)
    .execute(pool)
    .await?;
    Ok(r.rows_affected())
}
