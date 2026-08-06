//! T025 [US2] — `device_credentials`, `provisioning_claims` e
//! `residencia_secrets`. Estas tabelas são lidas APENAS aqui: nenhum endpoint de
//! cliente as expõe, e só o hash dos segredos é persistido (Princípio VIII).
use crate::dominio::{DeviceCredential, ProvisioningClaim};
use chrono::{Duration, NaiveDateTime, Utc};
use sqlx::{MySqlConnection, MySqlPool};

// ── device_credentials ──────────────────────────────────────────────────────

/// Grava o segredo durável (re-provisionar rotaciona e reativa a credencial).
pub async fn upsert_credencial(
    conn: &mut MySqlConnection,
    tomada_id: &str,
    secret_hash: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO device_credentials (tomada_id, secret_hash, criado_em) VALUES (?, ?, ?) \
         ON DUPLICATE KEY UPDATE secret_hash = VALUES(secret_hash), revoked_at = NULL",
    )
    .bind(tomada_id)
    .bind(secret_hash)
    .bind(Utc::now().naive_utc())
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn buscar_credencial(
    pool: &MySqlPool,
    tomada_id: &str,
) -> Result<Option<DeviceCredential>, sqlx::Error> {
    sqlx::query_as::<_, DeviceCredential>(
        "SELECT tomada_id, secret_hash, criado_em, revoked_at, ultima_renovacao \
         FROM device_credentials WHERE tomada_id = ?",
    )
    .bind(tomada_id)
    .fetch_optional(pool)
    .await
}

pub async fn marcar_renovacao(pool: &MySqlPool, tomada_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE device_credentials SET ultima_renovacao = ? WHERE tomada_id = ?")
        .bind(Utc::now().naive_utc())
        .bind(tomada_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Revogação (UC26): `/renovar` passa a falhar e o JWT vigente morre em ≤ TTL
/// (SC-008). Retorna linhas afetadas.
pub async fn revogar(conn: &mut MySqlConnection, tomada_id: &str) -> Result<u64, sqlx::Error> {
    let r = sqlx::query(
        "UPDATE device_credentials SET revoked_at = ? WHERE tomada_id = ? AND revoked_at IS NULL",
    )
    .bind(Utc::now().naive_utc())
    .bind(tomada_id)
    .execute(conn)
    .await?;
    Ok(r.rows_affected())
}

/// Rotaciona a chave de coordenação da residência (feature 005, FR-011).
///
/// Sem isto, revogar uma tomada cortava só o acesso dela ao servidor: ela seguia
/// conhecendo a chave da casa e continuava sendo aceita — e influenciando as decisões —
/// na malha ESP-NOW das irmãs. Chamada na MESMA transação do revoke.
pub async fn rotacionar_residencia_key(
    conn: &mut MySqlConnection,
    residencia_id: &str,
    gerar: impl Fn() -> String,
) -> Result<String, sqlx::Error> {
    let nova = gerar();
    sqlx::query(
        "INSERT INTO residencia_secrets (residencia_id, residencia_key, criado_em) \
         VALUES (?, ?, ?) ON DUPLICATE KEY UPDATE residencia_key = VALUES(residencia_key)",
    )
    .bind(residencia_id)
    .bind(&nova)
    .bind(Utc::now().naive_utc())
    .execute(conn)
    .await?;
    Ok(nova)
}

/// Chave corrente da residência (entregue ao device no `GET /config` — FR-012).
pub async fn buscar_residencia_key(
    pool: &MySqlPool,
    residencia_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    let r: Option<(String,)> =
        sqlx::query_as("SELECT residencia_key FROM residencia_secrets WHERE residencia_id = ?")
            .bind(residencia_id)
            .fetch_optional(pool)
            .await?;
    Ok(r.map(|(k,)| k))
}

// ── provisioning_claims ─────────────────────────────────────────────────────

pub async fn inserir_claim(
    pool: &MySqlPool,
    id: &str,
    residencia_id: &str,
    code_hash: &str,
    ttl_minutos: i64,
) -> Result<NaiveDateTime, sqlx::Error> {
    let agora = Utc::now().naive_utc();
    let expira_em = agora + Duration::minutes(ttl_minutos);
    sqlx::query(
        "INSERT INTO provisioning_claims (id, residencia_id, code_hash, expira_em, criado_em) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(residencia_id)
    .bind(code_hash)
    .bind(expira_em)
    .bind(agora)
    .execute(pool)
    .await?;
    Ok(expira_em)
}

pub async fn buscar_claim_por_hash(
    conn: &mut MySqlConnection,
    code_hash: &str,
) -> Result<Option<ProvisioningClaim>, sqlx::Error> {
    sqlx::query_as::<_, ProvisioningClaim>(
        "SELECT id, residencia_id, code_hash, expira_em, usado_em, criado_em \
         FROM provisioning_claims WHERE code_hash = ?",
    )
    .bind(code_hash)
    .fetch_optional(conn)
    .await
}

/// Consome o claim de forma atômica: o `WHERE usado_em IS NULL` é o que garante
/// uso único mesmo com dois devices tentando o mesmo código ao mesmo tempo.
/// Retorna `true` se ESTE chamador foi quem consumiu.
pub async fn consumir_claim(
    conn: &mut MySqlConnection,
    id: &str,
) -> Result<bool, sqlx::Error> {
    let r = sqlx::query(
        "UPDATE provisioning_claims SET usado_em = ? \
         WHERE id = ? AND usado_em IS NULL AND expira_em > ?",
    )
    .bind(Utc::now().naive_utc())
    .bind(id)
    .bind(Utc::now().naive_utc())
    .execute(conn)
    .await?;
    Ok(r.rows_affected() == 1)
}

// ── residencia_secrets ──────────────────────────────────────────────────────

/// Chave HMAC do ESP-NOW, compartilhada pelas tomadas da mesma residência.
/// Criada preguiçosamente na 1ª tomada provisionada e entregue só aqui.
pub async fn obter_ou_criar_residencia_key(
    conn: &mut MySqlConnection,
    residencia_id: &str,
    gerar: impl FnOnce() -> String,
) -> Result<String, sqlx::Error> {
    let existente: Option<(String,)> =
        sqlx::query_as("SELECT residencia_key FROM residencia_secrets WHERE residencia_id = ?")
            .bind(residencia_id)
            .fetch_optional(&mut *conn)
            .await?;
    if let Some((k,)) = existente {
        return Ok(k);
    }
    let chave = gerar();
    sqlx::query(
        "INSERT INTO residencia_secrets (residencia_id, residencia_key, criado_em) \
         VALUES (?, ?, ?) ON DUPLICATE KEY UPDATE residencia_key = residencia_key",
    )
    .bind(residencia_id)
    .bind(&chave)
    .bind(Utc::now().naive_utc())
    .execute(&mut *conn)
    .await?;
    // Releitura: se outra transação venceu a corrida, vale a chave dela.
    let (k,): (String,) =
        sqlx::query_as("SELECT residencia_key FROM residencia_secrets WHERE residencia_id = ?")
            .bind(residencia_id)
            .fetch_one(conn)
            .await?;
    Ok(k)
}
