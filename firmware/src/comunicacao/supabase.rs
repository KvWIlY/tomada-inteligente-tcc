//! Cliente Supabase (PostgREST) sob **JWT por-dispositivo** (feature 002).
//!
//! Contrato: `specs/002-refinamento-supabase/contracts/supabase-client.md`.
//! Regras transversais (todas as operacoes):
//!   - Headers: `apikey: <publishable>` (gateway) + `Authorization: Bearer <jwt device>`
//!     + `Content-Type` (+ `Prefer` conforme a operacao). NUNCA anon key.
//!   - JWT vem de `provisionamento::obter_jwt_valido()` (renova por margem); em `401/403`
//!     renova UMA vez e repete; nova falha => evento + erro (modo offline), sem travar.
//!   - TLS: cert bundle global; timeout 10 s.
//!   - `f32`: serializacao/parse manual (bug LLVM Xtensa). Inteiros/strings via serde_json.
//!   - `tomada_id`/`residencia_id` vem de `CredenciaisLocais`, nunca de constantes.

use anyhow::{anyhow, Context, Result};
use embedded_svc::http::client::Client;
use embedded_svc::http::Method;
use embedded_svc::io::Write as _;
use esp_idf_svc::http::client::{Configuration as HttpConfig, EspHttpConnection};
use std::sync::{Mutex, OnceLock};

use crate::comunicacao::provisionamento::{self, agora_epoch};
use crate::config_nuvem::{SUPABASE_PUBLISHABLE_KEY, SUPABASE_URL};
use crate::persistencia::config::Tomada;
use crate::persistencia::credenciais;
use crate::persistencia::medicao;
use tomada_solver::sync::{montar_corpo_lote, selecionar_pendentes};

/// Tamanho maximo do lote de medicoes por requisicao (cabe na RAM/resposta HTTP).
pub const MAX_LOTE: usize = 120;

// ============================================================
// Núcleo: requisicao autenticada com retry de renovacao em 401/403.
// ============================================================

fn http_config() -> HttpConfig {
    HttpConfig {
        use_global_ca_store: true,
        crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
        timeout: Some(std::time::Duration::from_secs(10)),
        // Supabase devolve muitos headers (CORS, rate-limit, CF...) e nosso request leva um
        // Bearer JWT longo: os 512 B default estouram ("HTTP_HEADER: Buffer length is
        // small to fit all the headers"). Folga no rx (resposta) e no tx (request).
        buffer_size: Some(4096),
        buffer_size_tx: Some(2048),
        ..Default::default()
    }
}

/// Envia UMA requisicao com o `jwt` dado. Retorna `(status, corpo)`.
fn enviar(
    metodo: Method,
    url: &str,
    jwt: &str,
    body: Option<&str>,
    prefer: Option<&str>,
) -> Result<(u16, String)> {
    let bearer = format!("Bearer {jwt}");
    let content_length = body.map(|b| b.len()).unwrap_or(0).to_string();

    // Monta headers (apikey publishable + Bearer JWT do device). NUNCA anon key.
    let mut headers: Vec<(&str, &str)> = vec![
        ("apikey", SUPABASE_PUBLISHABLE_KEY),
        ("Authorization", bearer.as_str()),
        ("Content-Type", "application/json"),
        ("Content-Length", content_length.as_str()),
    ];
    if let Some(p) = prefer {
        headers.push(("Prefer", p));
    }

    let mut client = Client::wrap(EspHttpConnection::new(&http_config())?);
    let mut req = client.request(metodo, url, &headers)?;
    if let Some(b) = body {
        req.write_all(b.as_bytes())?;
        req.flush()?;
    }
    let mut resp = req.submit()?;
    let status = resp.status();

    let mut out = Vec::new();
    let mut buf = [0u8; 1024];
    loop {
        match resp.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(e) => return Err(anyhow!("[SUPABASE] leitura da resposta falhou: {e:?}")),
        }
    }
    Ok((status, String::from_utf8_lossy(&out).to_string()))
}

/// Requisicao autenticada com **renovacao reativa**: obtem JWT valido, envia; em `401/403`
/// renova UMA vez e repete. Falha persistente => `auth_revogada`/`auth_falha` + erro.
fn requisicao(
    metodo: Method,
    url: &str,
    body: Option<&str>,
    prefer: Option<&str>,
) -> Result<(u16, String)> {
    let jwt = provisionamento::obter_jwt_valido()?;
    let (status, corpo) = enviar(metodo, url, &jwt, body, prefer)?;
    if status != 401 && status != 403 {
        return Ok((status, corpo));
    }
    // 401/403: tenta renovar 1x e repetir.
    log::warn!("[SUPABASE] {status} em {url} — renovando JWT e repetindo");
    let jwt2 = match provisionamento::renovar_agora() {
        Ok(j) => j,
        Err(e) => {
            registrar_evento("auth_revogada", "erro", "renovacao falhou", &format!("{e}"));
            return Err(anyhow!("[SUPABASE] renovacao falhou apos {status}: {e}"));
        }
    };
    let (status2, corpo2) = enviar(metodo, url, &jwt2, body, prefer)?;
    if status2 == 401 || status2 == 403 {
        registrar_evento("auth_falha", "warn", "401/403 persistente", url);
        return Err(anyhow!("[SUPABASE] {status2} persistente em {url} apos renovar"));
    }
    Ok((status2, corpo2))
}

fn tomada_id() -> Result<String> {
    Ok(credenciais::load()?
        .context("sem credencial provisionada")?
        .tomada_id)
}

fn residencia_id() -> Result<String> {
    Ok(credenciais::load()?
        .context("sem credencial provisionada")?
        .residencia_id)
}

// ============================================================
// Escrita: sync em lote de medicoes (idempotente).
// ============================================================

/// Envia em LOTE as medicoes pendentes (`confirmado=false`). Em 2xx marca todas como
/// `confirmado=true` numa UNICA consolidacao na flash (FR-010/011/013, Principio III).
/// Reenvio e seguro: `on_conflict=tomada_id,timestamp` + `resolution=ignore-duplicates`
/// casa no `unique (tomada_id, timestamp)` do schema. Retorna quantas confirmou.
pub fn sincronizar_medicoes() -> Result<usize> {
    let pendentes = medicao::carregar_pendentes()?;
    if pendentes.is_empty() {
        return Ok(0);
    }
    let registros: Vec<_> = pendentes.iter().map(|m| m.para_sync_record()).collect();
    let idx = selecionar_pendentes(&registros, MAX_LOTE);
    if idx.is_empty() {
        return Ok(0);
    }
    let corpo = montar_corpo_lote(&registros, &idx);
    let url = format!("{SUPABASE_URL}/rest/v1/medicoes?on_conflict=tomada_id,timestamp");
    let (status, body) = requisicao(
        Method::Post,
        &url,
        Some(&corpo),
        Some("return=minimal,resolution=ignore-duplicates"),
    )?;
    if (200..=299).contains(&status) {
        let ts_ok: Vec<i64> = idx.iter().map(|&i| registros[i].timestamp).collect();
        let n = ts_ok.len();
        medicao::marcar_confirmados(&ts_ok)?;
        log::info!("[SUPABASE] {n} medicoes sincronizadas e confirmadas");
        Ok(n)
    } else if status == 404 {
        registrar_evento("schema_ausente", "erro", "medicoes 404", &body);
        Err(anyhow!("[SUPABASE] medicoes HTTP 404 (schema ausente?)"))
    } else {
        Err(anyhow!("[SUPABASE] sincronizar_medicoes HTTP {status}: {body}"))
    }
}

// ============================================================
// Escrita: PATCH na propria linha de `tomadas` (colunas permitidas ao device).
// ============================================================

fn patch_tomada(body: &str, ctx: &str) -> Result<()> {
    let id = tomada_id()?;
    let url = format!("{SUPABASE_URL}/rest/v1/tomadas?id=eq.{id}");
    let (status, resp) = requisicao(Method::Patch, &url, Some(body), Some("return=minimal"))?;
    if (200..=299).contains(&status) {
        Ok(())
    } else {
        Err(anyhow!("[SUPABASE] {ctx} HTTP {status}: {resp}"))
    }
}

/// Publica o estado decidido pelo solver. NAO toca `rele_aberto` (comando do dashboard).
pub fn atualizar_estado(estado: &str) -> Result<()> {
    patch_tomada(&format!(r#"{{"estado":"{estado}"}}"#), "atualizar_estado")
}

/// Publica a calibracao de consumo (`C_{{i,STD}}`/`C_{{i,ON}}`). f32 manual.
pub fn atualizar_calibracao(cstd: f32, con: f32) -> Result<()> {
    let body = format!(r#"{{"cstd":{cstd:.2},"con":{con:.2}}}"#);
    patch_tomada(&body, "atualizar_calibracao")
}

/// Publica a telemetria corrente (compartilhada via nuvem p/ fallback multi-no). f32 manual.
pub fn atualizar_telemetria(p_atual: f32, ultimo_uso_ativo_ts: i64) -> Result<()> {
    let body = format!(
        r#"{{"p_medida_atual":{p_atual:.2},"ultimo_uso_ativo_ts":{ultimo_uso_ativo_ts}}}"#
    );
    patch_tomada(&body, "atualizar_telemetria")
}

// ============================================================
// Escrita: eventos (observabilidade, Principio VII / FR-017). Fila em RAM + flush em lote.
// ============================================================

struct EventoPendente {
    timestamp: i64,
    tipo: String,
    severidade: String,
    titulo: String,
    detalhes: String,
}

fn fila_eventos() -> &'static Mutex<Vec<EventoPendente>> {
    static F: OnceLock<Mutex<Vec<EventoPendente>>> = OnceLock::new();
    F.get_or_init(|| Mutex::new(Vec::new()))
}

/// Enfileira um evento (NUNCA inclui segredo/jwt/claims — Principio VII/VIII).
pub fn registrar_evento(tipo: &str, severidade: &str, titulo: &str, detalhes: &str) {
    fila_eventos().lock().unwrap().push(EventoPendente {
        timestamp: agora_epoch(),
        tipo: tipo.to_string(),
        severidade: severidade.to_string(),
        titulo: titulo.to_string(),
        detalhes: detalhes.to_string(),
    });
}

/// Envia em lote os eventos enfileirados. Em 2xx limpa a fila; senao preserva p/ retry.
pub fn sincronizar_eventos() -> Result<usize> {
    let snapshot: Vec<String> = {
        let fila = fila_eventos().lock().unwrap();
        if fila.is_empty() {
            return Ok(0);
        }
        let id = tomada_id()?;
        fila.iter()
            .map(|e| {
                format!(
                    r#"{{"tomada_id":"{}","timestamp":{},"tipo":"{}","severidade":"{}","titulo":"{}","detalhes":"{}"}}"#,
                    id,
                    e.timestamp,
                    escape_json(&e.tipo),
                    escape_json(&e.severidade),
                    escape_json(&e.titulo),
                    escape_json(&e.detalhes),
                )
            })
            .collect()
    };
    let corpo = format!("[{}]", snapshot.join(","));
    let url = format!("{SUPABASE_URL}/rest/v1/eventos");
    let (status, body) = requisicao(Method::Post, &url, Some(&corpo), Some("return=minimal"))?;
    if (200..=299).contains(&status) {
        let n = snapshot.len();
        fila_eventos().lock().unwrap().clear();
        log::info!("[SUPABASE] {n} eventos sincronizados");
        Ok(n)
    } else {
        Err(anyhow!("[SUPABASE] sincronizar_eventos HTTP {status}: {body}"))
    }
}

// ============================================================
// Leitura: desired state (pull de config + comando manual de rele).
// ============================================================

/// Estado desejado lido da nuvem (`buscar_desired_state`).
pub struct DesiredState {
    /// Config remota (com `config_version` preenchido). Adocao decidida no ciclo
    /// via `tomada_solver::sync::deve_adotar_config` (so se versao remota > local).
    pub config: Option<Tomada>,
    /// Comando manual do dashboard. Entregue ao ciclo SUBORDINADO ao P_max (FR-016).
    pub rele_aberto: Option<bool>,
}

/// GET `select=config,config_version,rele_aberto` da propria tomada (FR-015/016).
pub fn buscar_desired_state() -> Result<DesiredState> {
    let id = tomada_id()?;
    let url = format!(
        "{SUPABASE_URL}/rest/v1/tomadas?id=eq.{id}&select=config,config_version,rele_aberto"
    );
    let (status, body) = requisicao(Method::Get, &url, None, None)?;
    if status == 404 {
        registrar_evento("schema_ausente", "erro", "tomadas 404", "buscar_desired_state");
        return Err(anyhow!("[SUPABASE] buscar_desired_state HTTP 404"));
    }
    if !(200..=299).contains(&status) {
        return Err(anyhow!("[SUPABASE] buscar_desired_state HTTP {status}: {body}"));
    }

    let rele_aberto = if body.contains("\"rele_aberto\":true") {
        Some(true)
    } else if body.contains("\"rele_aberto\":false") {
        Some(false)
    } else {
        None
    };

    let config = match (extrair_json_config(&body), extrair_i64(&body, "config_version")) {
        (Some(cfg_json), Some(versao)) => match serde_json::from_str::<Tomada>(cfg_json) {
            Ok(mut c) => {
                c.config_version = versao;
                Some(c)
            }
            Err(e) => {
                log::warn!("[SUPABASE] config remota nao parseou: {e}");
                None
            }
        },
        _ => None,
    };

    Ok(DesiredState { config, rele_aberto })
}

/// GET do `P_max` canonico da residencia (`residencias.p_max`) — a FONTE DE VERDADE do
/// teto eletrico, editavel no dashboard (slider). O `config.p_max` do jsonb da tomada NAO
/// e fonte (o dashboard nem o escreve; o default do firmware e so fallback offline). O
/// ciclo usa este valor no solver e o persiste localmente (offline-first, Principio VI).
/// `Ok(None)` se a residencia nao retornou linha (ex.: RLS/ausente).
pub fn buscar_p_max_residencia() -> Result<Option<f32>> {
    let rid = residencia_id()?;
    let url = format!("{SUPABASE_URL}/rest/v1/residencias?id=eq.{rid}&select=p_max");
    let (status, body) = requisicao(Method::Get, &url, None, None)?;
    if !(200..=299).contains(&status) {
        return Err(anyhow!("[SUPABASE] buscar_p_max_residencia HTTP {status}: {body}"));
    }
    // Corpo `[{"p_max":3000}]` — unico campo, `extrair_i64` e inequivoco aqui.
    Ok(extrair_i64(&body, "p_max").map(|v| v as f32))
}

// ============================================================
// Helpers de parse (sem serde p/ campos simples).
// ============================================================

fn extrair_i64(body: &str, chave: &str) -> Option<i64> {
    let key = format!("\"{chave}\":");
    let pos = body.find(&key)? + key.len();
    let resto = body[pos..].trim_start();
    let fim = resto
        .find(|c: char| !c.is_ascii_digit() && c != '-')
        .unwrap_or(resto.len());
    resto[..fim].parse().ok()
}

/// Extrai a substring do objeto jsonb `"config":{...}` (conta chaves balanceadas).
fn extrair_json_config(body: &str) -> Option<&str> {
    let marca = "\"config\":";
    let inicio = body.find(marca)? + marca.len();
    let slice = body[inicio..].trim_start();
    if !slice.starts_with('{') {
        return None;
    }
    let mut depth = 0i32;
    for (i, c) in slice.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&slice[..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

fn escape_json(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

// ============================================================
// Leitura: todas as tomadas da residencia (fallback multi-no pela nuvem).
// ============================================================

/// Dado remoto de uma tomada-irma (parseado do GET). Fallback quando ESP-NOW nao cobre.
pub struct TomadaRemota {
    pub id: String,
    pub p_medida_atual: f32,
    pub ultimo_uso_ativo_ts: i64,
}

/// GET das tomadas da residencia (RLS filtra por `residencia_id` do claim). Fallback de
/// coordenacao quando o ESP-NOW esta incompleto; processa so os campos numericos simples.
pub fn buscar_todas_tomadas() -> Result<Vec<TomadaRemota>> {
    let rid = residencia_id()?;
    let url = format!(
        "{SUPABASE_URL}/rest/v1/tomadas?residencia_id=eq.{rid}&select=id,p_medida_atual,ultimo_uso_ativo_ts"
    );
    let (status, body) = requisicao(Method::Get, &url, None, None)?;
    if !(200..=299).contains(&status) {
        return Err(anyhow!("[SUPABASE] buscar_todas_tomadas HTTP {status}: {body}"));
    }
    #[derive(serde::Deserialize)]
    struct Row {
        id: String,
        #[serde(default)]
        p_medida_atual: f32,
        #[serde(default)]
        ultimo_uso_ativo_ts: i64,
    }
    let rows: Vec<Row> = serde_json::from_str(&body).context("parse buscar_todas_tomadas")?;
    Ok(rows
        .into_iter()
        .map(|r| TomadaRemota {
            id: r.id,
            p_medida_atual: r.p_medida_atual,
            ultimo_uso_ativo_ts: r.ultimo_uso_ativo_ts,
        })
        .collect())
}
