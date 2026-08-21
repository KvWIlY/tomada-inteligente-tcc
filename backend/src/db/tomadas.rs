//! T024 [US2] — queries de `tomadas`.
//!
//! Regra de autorização que esta camada respeita (ver notas do schema): o device
//! só escreve as colunas do PRÓPRIO nó (`estado`, `cstd`, `con`,
//! `p_medida_atual`, `ultimo_uso_ativo_ts`); `rele_aberto`, `mac`,
//! `residencia_id` e `p_max` são território do dono.
use crate::dominio::Tomada;
use chrono::Utc;
use serde::Serialize;
use sqlx::{MySqlConnection, MySqlPool};

const COLS: &str = "id, residencia_id, mac, nome, estado, rele_aberto, cstd, con, \
                    p_medida_atual, ultimo_uso_ativo_ts, config, config_version, criado_em";

pub async fn buscar_por_id(pool: &MySqlPool, id: &str) -> Result<Option<Tomada>, sqlx::Error> {
    sqlx::query_as::<_, Tomada>(&format!("SELECT {COLS} FROM tomadas WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn buscar_por_mac(
    conn: &mut MySqlConnection,
    mac: &str,
) -> Result<Option<Tomada>, sqlx::Error> {
    sqlx::query_as::<_, Tomada>(&format!("SELECT {COLS} FROM tomadas WHERE mac = ?"))
        .bind(mac)
        .fetch_optional(conn)
        .await
}

pub async fn listar_por_residencia(
    pool: &MySqlPool,
    residencia_id: &str,
) -> Result<Vec<Tomada>, sqlx::Error> {
    sqlx::query_as::<_, Tomada>(&format!(
        "SELECT {COLS} FROM tomadas WHERE residencia_id = ? ORDER BY criado_em"
    ))
    .bind(residencia_id)
    .fetch_all(pool)
    .await
}

/// Insere uma tomada nova no provisionamento. `config` nasce `{}` — o device
/// puxa o desired-state em `GET /config` e o dono preenche pelo dashboard.
pub async fn inserir(
    conn: &mut MySqlConnection,
    id: &str,
    residencia_id: &str,
    mac: &str,
    nome: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO tomadas (id, residencia_id, mac, nome, config, criado_em) \
         VALUES (?, ?, ?, ?, '{}', ?)",
    )
    .bind(id)
    .bind(residencia_id)
    .bind(mac)
    .bind(nome)
    .bind(Utc::now().naive_utc())
    .execute(conn)
    .await?;
    Ok(())
}

/// Snapshot instantâneo escrito pelo próprio nó junto com o lote de telemetria.
/// `cstd`/`con` são a calibração e podem vir ausentes (COALESCE mantém o valor).
pub struct Snapshot {
    pub p_medida_atual: f32,
    pub ultimo_uso_ativo_ts: i64,
    pub estado: String,
    pub cstd: Option<f32>,
    pub con: Option<f32>,
    /// Fim da janela de "usar agora": devolve `rele_aberto` a NULL (automático).
    pub liberar_manual: bool,
}

pub async fn atualizar_snapshot(
    conn: &mut MySqlConnection,
    tomada_id: &str,
    s: &Snapshot,
) -> Result<(), sqlx::Error> {
    // `liberar_manual` só sabe LIMPAR: com a flag em 0 a coluna fica intocada, e o
    // device nunca consegue escrever `true`/`false` nela. Uma ordem do dono emitida
    // enquanto o POST estava em voo sobrevive — a única perda possível é o próprio
    // comando que a janela já estava consumindo.
    sqlx::query(
        "UPDATE tomadas SET p_medida_atual = ?, ultimo_uso_ativo_ts = ?, estado = ?, \
         cstd = COALESCE(?, cstd), con = COALESCE(?, con), \
         rele_aberto = CASE WHEN ? THEN NULL ELSE rele_aberto END WHERE id = ?",
    )
    .bind(s.p_medida_atual)
    .bind(s.ultimo_uso_ativo_ts)
    .bind(&s.estado)
    .bind(s.cstd)
    .bind(s.con)
    .bind(s.liberar_manual as i8)
    .bind(tomada_id)
    .execute(conn)
    .await?;
    Ok(())
}

/// Carga instantânea das **irmãs** (US4/UC22): exclui a própria tomada. O
/// backend só devolve dados — quem decide frescor e soma é o firmware (FR-016).
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ConsumoIrma {
    pub tomada_id: String,
    pub p_medida_atual: f32,
    pub ultimo_uso_ativo_ts: i64,
}

pub async fn consumo_irmas(
    pool: &MySqlPool,
    residencia_id: &str,
    excluir_tomada_id: &str,
) -> Result<Vec<ConsumoIrma>, sqlx::Error> {
    sqlx::query_as::<_, ConsumoIrma>(
        "SELECT id AS tomada_id, p_medida_atual, ultimo_uso_ativo_ts \
         FROM tomadas WHERE residencia_id = ? AND id <> ?",
    )
    .bind(residencia_id)
    .bind(excluir_tomada_id)
    .fetch_all(pool)
    .await
}

// ── Escrita do dono (US3) ───────────────────────────────────────────────────

/// PATCH do dashboard. `config` é o único campo que bumpa `config_version` — é
/// esse número que o nó compara para decidir se re-aplica o desired-state
/// (UC28); renomear ou mexer no relé não deve forçar re-leitura de config.
/// `rele_aberto`: `None` = não mexe; `Some(None)` = volta ao automático (NULL);
/// `Some(Some(b))` = grava o comando. `COALESCE` não serve aqui — ele não consegue
/// expressar "grave NULL de propósito", que é exatamente o valor de "automático".
pub async fn atualizar(
    pool: &MySqlPool,
    id: &str,
    nome: Option<&str>,
    config: Option<&str>,
    rele_aberto: Option<Option<bool>>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE tomadas SET \
           nome = COALESCE(?, nome), \
           config = COALESCE(CAST(? AS JSON), config), \
           config_version = config_version + (? IS NOT NULL), \
           rele_aberto = CASE WHEN ? THEN ? ELSE rele_aberto END \
         WHERE id = ?",
    )
    .bind(nome)
    .bind(config)
    .bind(config)
    .bind(rele_aberto.is_some() as i8)
    .bind(rele_aberto.flatten().map(|b| b as i8))
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

// ── Visão geral da residência (T041/UC04) ───────────────────────────────────

/// Uma linha por tomada calibrada, com o que o score de eficiência precisa.
#[derive(sqlx::FromRow)]
struct LinhaScore {
    cstd: f32,
    amostras: i64,
    ociosas: Option<i64>,
    desligadas: Option<i64>,
    ts_min: Option<i64>,
    ts_max: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct VisaoGeral {
    pub tomadas: i64,
    pub consumo_total_atual: f64,
    /// `None` quando nenhuma tomada tem standby calibrado — sem base para o
    /// cálculo, a UI mostra "—" em vez de inventar um número.
    pub efficiency_score: Option<i64>,
}

pub async fn visao_geral(
    pool: &MySqlPool,
    residencia_id: &str,
    desde_ts: i64,
) -> Result<VisaoGeral, sqlx::Error> {
    let (tomadas, consumo_total_atual): (i64, f64) = sqlx::query_as(
        "SELECT COUNT(*), \
                COALESCE(SUM(CASE WHEN estado <> 'OFF' THEN p_medida_atual ELSE 0 END), 0) \
         FROM tomadas WHERE residencia_id = ?",
    )
    .bind(residencia_id)
    .fetch_one(pool)
    .await?;

    // Mesma definição do dashboard (frontend/src/derive.ts): de toda a energia
    // fantasma que poderia ter sido gasta enquanto a tomada estava ociosa
    // (estado <> ON), quanto % foi evitada abrindo o relé (estado = OFF).
    let linhas = sqlx::query_as::<_, LinhaScore>(
        "SELECT t.cstd AS cstd, \
                COUNT(m.id) AS amostras, \
                CAST(SUM(m.estado <> 'ON') AS SIGNED) AS ociosas, \
                CAST(SUM(m.estado = 'OFF') AS SIGNED) AS desligadas, \
                MIN(m.timestamp) AS ts_min, MAX(m.timestamp) AS ts_max \
         FROM tomadas t \
         LEFT JOIN medicoes m ON m.tomada_id = t.id AND m.timestamp >= ? \
         WHERE t.residencia_id = ? AND t.cstd > 0 \
         GROUP BY t.id, t.cstd",
    )
    .bind(desde_ts)
    .bind(residencia_id)
    .fetch_all(pool)
    .await?;

    let mut evitada = 0.0f64;
    let mut potencial = 0.0f64;
    for l in linhas {
        // Intervalo médio entre amostras, em horas (idem `calcIntervaloHoras`).
        let ih = match (l.amostras, l.ts_min, l.ts_max) {
            (n, Some(min), Some(max)) if n >= 2 => (max - min) as f64 / (n - 1) as f64 / 3600.0,
            _ => 1.0 / 60.0,
        };
        let cstd = l.cstd as f64;
        evitada += l.desligadas.unwrap_or(0) as f64 * cstd * ih;
        potencial += l.ociosas.unwrap_or(0) as f64 * cstd * ih;
    }
    let efficiency_score = if potencial > 0.0 {
        Some((evitada / potencial * 100.0).round() as i64)
    } else {
        None
    };

    Ok(VisaoGeral {
        tomadas,
        consumo_total_atual,
        efficiency_score,
    })
}
