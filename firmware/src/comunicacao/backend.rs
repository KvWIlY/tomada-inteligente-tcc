//! Cliente do **backend proprio** (Actix-Web) sob JWT por-dispositivo (feature 004).
//!
//! Contrato:
//! `specs/004-backend-actix-mysql/contracts/backend-api.md`.
//!
//! Regras transversais (todas as operacoes):
//!   - Header unico de auth: `Authorization: Bearer <jwt device>`. NAO existe mais
//!     `apikey` — o backend proprio nao tem gateway na frente.
//!   - JWT vem de `provisionamento::obter_jwt_valido()` (renova por margem); em
//!     `401/403` renova UMA vez e repete; nova falha => evento + erro (modo offline),
//!     sem travar o ciclo decisorio (Principio III).
//!   - O escopo (`tomada_id`/`residencia_id`) vive no claim do JWT: o backend ignora
//!     qualquer id que venha no corpo. Nao ha como este cliente tocar outra tomada.
//!   - TLS: cert bundle global (LAN-only hoje, HTTPS pronto p/ depois); timeout 10 s.
//!   - `f32`: serializacao manual (`{:.2}`) — bug LLVM Xtensa com `[2 x float]`.
//!
//! Sao apenas 2 requisicoes por ciclo de nuvem: um `POST /telemetria` que carrega o
//! lote de medicoes + eventos + snapshot, e um `GET /config` que ja traz o `p_max` da
//! residencia junto com o desired-state.

use anyhow::{anyhow, Context, Result};
use embedded_svc::http::client::Client;
use embedded_svc::http::Method;
use embedded_svc::io::Write as _;
use esp_idf_svc::http::client::{Configuration as HttpConfig, EspHttpConnection};
use serde::Deserialize;
use std::sync::{Mutex, OnceLock};

use crate::comunicacao::provisionamento::{self, agora_epoch};
use crate::comunicacao::sntp;
use crate::config_nuvem::backend_url;
use crate::persistencia::config::Tomada;
use crate::persistencia::credenciais;
use crate::persistencia::medicao;
use crate::solver::relogio::{self, Carimbavel};
use crate::solver::sync::{montar_corpo_lote, selecionar_pendentes};

/// Tamanho maximo do lote de medicoes por requisicao (cabe na RAM/resposta HTTP).
pub const MAX_LOTE: usize = 120;

// ============================================================
// Nucleo: requisicao autenticada com retry de renovacao em 401/403.
// ============================================================

fn http_config() -> HttpConfig {
    HttpConfig {
        use_global_ca_store: true,
        crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
        timeout: Some(std::time::Duration::from_secs(10)),
        // O Bearer JWT e longo; 512 B default estouram ("HTTP_HEADER: Buffer length
        // is small to fit all the headers"). Folga no rx e no tx.
        buffer_size: Some(4096),
        buffer_size_tx: Some(2048),
        ..Default::default()
    }
}

/// Envia UMA requisicao com o `jwt` dado. Retorna `(status, corpo)`.
fn enviar(metodo: Method, url: &str, jwt: &str, body: Option<&str>) -> Result<(u16, String)> {
    let bearer = format!("Bearer {jwt}");
    let content_length = body.map(|b| b.len()).unwrap_or(0).to_string();
    let headers: [(&str, &str); 3] = [
        ("Authorization", bearer.as_str()),
        ("Content-Type", "application/json"),
        ("Content-Length", content_length.as_str()),
    ];

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
            Err(e) => return Err(anyhow!("[BACKEND] leitura da resposta falhou: {e:?}")),
        }
    }
    Ok((status, String::from_utf8_lossy(&out).to_string()))
}

/// Requisicao autenticada com **renovacao reativa**: obtem JWT valido, envia; em
/// `401/403` renova UMA vez e repete. Falha persistente => `auth_revogada`/`auth_falha`.
fn requisicao(metodo: Method, url: &str, body: Option<&str>) -> Result<(u16, String)> {
    let jwt = provisionamento::obter_jwt_valido()?;
    let (status, corpo) = enviar(metodo, url, &jwt, body)?;
    if status != 401 && status != 403 {
        return Ok((status, corpo));
    }
    log::warn!("[BACKEND] {status} em {url} — renovando JWT e repetindo");
    let jwt2 = match provisionamento::renovar_agora() {
        Ok(j) => j,
        Err(e) => {
            registrar_evento("auth_revogada", "erro", "renovacao falhou", &format!("{e}"));
            return Err(anyhow!("[BACKEND] renovacao falhou apos {status}: {e}"));
        }
    };
    let (status2, corpo2) = enviar(metodo, url, &jwt2, body)?;
    if status2 == 401 || status2 == 403 {
        registrar_evento("auth_falha", "warn", "401/403 persistente", url);
        return Err(anyhow!("[BACKEND] {status2} persistente em {url} apos renovar"));
    }
    Ok((status2, corpo2))
}

fn residencia_id() -> Result<String> {
    Ok(credenciais::load()?
        .context("sem credencial provisionada")?
        .residencia_id)
}

// ============================================================
// Snapshot do no (estado instantaneo publicado junto com o lote).
// ============================================================

struct Snapshot {
    p_medida_atual: f32,
    ultimo_uso_ativo_ts: i64,
    estado: String,
    cstd: f32,
    con: f32,
    /// Pede ao servidor para devolver `rele_aberto` a NULL (automatico). E o fim de
    /// uma janela de "usar agora": sem isso o `GET /config` seguinte repetiria o
    /// comando e a tomada nunca sairia do modo manual.
    liberar_manual: bool,
}

fn snapshot_atual() -> &'static Mutex<Option<Snapshot>> {
    static S: OnceLock<Mutex<Option<Snapshot>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(None))
}

/// Registra o estado instantaneo decidido pelo ciclo. Substitui os tres PATCH
/// que a versao anterior fazia (`atualizar_telemetria`/`atualizar_estado`/
/// `atualizar_calibracao`): agora tudo viaja no `snapshot` do proximo
/// `POST /telemetria`, numa requisicao so.
///
/// `ultimo_uso_ativo_ts` e decidido AQUI (avanca so quando `U_i` e verdadeiro, ver
/// `decisao::ciclo`) — o backend apenas persiste, nunca infere. E o que as irmas
/// leem no fallback de particao (UC22).
pub fn registrar_snapshot(
    p_medida_atual: f32,
    ultimo_uso_ativo_ts: i64,
    estado: &str,
    cstd: f32,
    con: f32,
    liberar_manual: bool,
) {
    *snapshot_atual().lock().unwrap() = Some(Snapshot {
        p_medida_atual,
        ultimo_uso_ativo_ts,
        estado: estado.to_string(),
        cstd,
        con,
        liberar_manual,
    });
}

// ============================================================
// Eventos (observabilidade, Principio VII). Fila em RAM, drenada no sync.
// ============================================================

struct EventoPendente {
    /// Epoch real, ou `relogio::SEM_HORA` (0) enquanto nao houve sincronizacao —
    /// corrigido por deslocamento quando a hora chega, igual as medicoes (FR-001a).
    timestamp: i64,
    /// Segundos desde o boot no momento do registro: e o que permite datar o evento
    /// retroativamente. A fila e so RAM, entao todo evento aqui e DESTE boot e sempre
    /// corrigivel — nao existe o caso "incorrigivel de boot anterior" das medicoes.
    uptime_s: i64,
    tipo: String,
    severidade: String,
    titulo: String,
    detalhes: String,
}

/// Permite ao `crate::solver::relogio` corrigir os carimbos sem conhecer o tipo.
impl Carimbavel for EventoPendente {
    fn ts(&self) -> i64 {
        self.timestamp
    }
    fn uptime_s(&self) -> i64 {
        self.uptime_s
    }
    fn set_ts(&mut self, ts: i64) {
        self.timestamp = ts;
    }
}

fn fila_eventos() -> &'static Mutex<Vec<EventoPendente>> {
    static F: OnceLock<Mutex<Vec<EventoPendente>>> = OnceLock::new();
    F.get_or_init(|| Mutex::new(Vec::new()))
}

/// Teto da fila de eventos em RAM: offline prolongado gera eventos a cada ciclo e a
/// fila NAO pode crescer sem limite (OOM). Cheia => descarta o mais ANTIGO.
const MAX_FILA_EVENTOS: usize = 200;

/// Enfileira um evento (NUNCA inclui segredo/jwt/claims — Principio VII/VIII).
pub fn registrar_evento(tipo: &str, severidade: &str, titulo: &str, detalhes: &str) {
    let mut fila = fila_eventos().lock().unwrap();
    if fila.len() >= MAX_FILA_EVENTOS {
        fila.remove(0);
    }
    // Sem hora certa, carimba com a sentinela: `agora_epoch()` devolveria o UPTIME, que
    // o backend guardaria como uma data em 1970 (visto em producao em 2026-08-19).
    fila.push(EventoPendente {
        timestamp: if sntp::relogio_ok() {
            agora_epoch()
        } else {
            relogio::SEM_HORA
        },
        uptime_s: sntp::uptime_s(),
        tipo: tipo.to_string(),
        severidade: severidade.to_string(),
        titulo: titulo.to_string(),
        detalhes: detalhes.to_string(),
    });
}

/// Aplica o deslocamento do 1o sync SNTP aos eventos ainda na fila (FR-001a).
/// Devolve quantos foram datados.
pub fn corrigir_carimbos_eventos(offset: i64) -> usize {
    let mut fila = fila_eventos().lock().unwrap();
    relogio::corrigir(&mut fila[..], offset).corrigidos
}

// ============================================================
// Escrita: POST /telemetria (lote idempotente + eventos + snapshot).
// ============================================================

/// Envia num unico POST: medicoes pendentes, eventos enfileirados e o snapshot do no.
///
/// Idempotencia (FR-009): reenviar o mesmo lote nao duplica nada no servidor
/// (`UNIQUE(tomada_id, timestamp)` + dedup de eventos). Em 2xx o servidor devolve
/// `confirmado_ate_ts` e so ai marcamos as medicoes como confirmadas e drenamos a
/// fila de eventos (FR-011/UC18) — falha de rede nunca perde dado, so adia.
/// Retorna quantas medicoes foram confirmadas.
pub fn sincronizar() -> Result<usize> {
    let pendentes = medicao::carregar_pendentes()?;
    let registros: Vec<_> = pendentes.iter().map(|m| m.para_sync_record()).collect();
    let idx = selecionar_pendentes(&registros, MAX_LOTE);

    // `montar_corpo_lote` (logica pura de 002, inalterada) inclui `tomada_id` em cada
    // registro; o backend ignora esse campo e usa o claim do JWT.
    let medicoes_json = montar_corpo_lote(&registros, &idx);
    let eventos_json = {
        let fila = fila_eventos().lock().unwrap();
        // Sobrou evento sem hora certa? Nao manda NENHUM nesta rodada. Eles estao no
        // inicio da fila (a hora so passa a existir uma vez), e o `drain` la embaixo
        // assume que os N enviados sao exatamente os N primeiros — filtrar no meio
        // drenaria os errados. Sobem no proximo tick, ja datados. E a mesma regra das
        // medicoes, que tambem nunca sobem com a sentinela (FR-001a).
        if fila.iter().any(|e| e.timestamp == relogio::SEM_HORA) {
            log::info!("[BACKEND] {} evento(s) aguardando hora certa", fila.len());
            (String::from("[]"), 0usize)
        } else {
        let itens: Vec<String> = fila
            .iter()
            .map(|e| {
                format!(
                    r#"{{"timestamp":{},"tipo":"{}","severidade":"{}","titulo":"{}","detalhes":"{}"}}"#,
                    e.timestamp,
                    escape_json(&e.tipo),
                    escape_json(&e.severidade),
                    escape_json(&e.titulo),
                    escape_json(&e.detalhes),
                )
            })
            .collect();
        (format!("[{}]", itens.join(",")), itens.len())
        }
    };
    let (eventos_json, n_eventos) = eventos_json;

    let snapshot_json = snapshot_atual().lock().unwrap().as_ref().map(|s| {
        // f32 formatado a mao (bug LLVM Xtensa).
        format!(
            r#","snapshot":{{"p_medida_atual":{:.2},"ultimo_uso_ativo_ts":{},"estado":"{}","cstd":{:.2},"con":{:.2},"liberar_manual":{}}}"#,
            s.p_medida_atual, s.ultimo_uso_ativo_ts, s.estado, s.cstd, s.con, s.liberar_manual
        )
    });

    // Nada a dizer? Nao gasta radio.
    if idx.is_empty() && n_eventos == 0 && snapshot_json.is_none() {
        return Ok(0);
    }

    let corpo = format!(
        r#"{{"medicoes":{medicoes_json},"eventos":{eventos_json}{}}}"#,
        snapshot_json.unwrap_or_default()
    );
    let url = format!("{}/telemetria", backend_url());
    let (status, body) = requisicao(Method::Post, &url, Some(&corpo))?;
    if !(200..=299).contains(&status) {
        return Err(anyhow!("[BACKEND] telemetria HTTP {status}: {body}"));
    }

    // Confirma so ate onde o servidor garantiu ter persistido.
    let confirmado_ate = extrair_i64(&body, "confirmado_ate_ts").unwrap_or(i64::MIN);
    let ts_ok: Vec<i64> = idx
        .iter()
        .map(|&i| registros[i].timestamp)
        .filter(|&ts| ts <= confirmado_ate)
        .collect();
    let n = ts_ok.len();
    if n > 0 {
        medicao::marcar_confirmados(&ts_ok)?;
    }

    // Drena so os N eventos enviados: os enfileirados DURANTE o POST sobrevivem.
    if n_eventos > 0 {
        let mut fila = fila_eventos().lock().unwrap();
        let k = n_eventos.min(fila.len());
        fila.drain(..k);
    }
    log::info!("[BACKEND] sync: {n} medicoes confirmadas, {n_eventos} eventos");
    Ok(n)
}

// ============================================================
// Leitura: GET /config (desired state + P_max da residencia).
// ============================================================

/// Estado desejado lido do backend.
pub struct DesiredState {
    /// Config remota (com `config_version` preenchido). Adocao decidida no ciclo via
    /// `crate::solver::sync::deve_adotar_config` (so se versao remota > local).
    pub config: Option<Tomada>,
    /// Comando manual do dashboard, TRI-ESTADO (migration 0002):
    ///   `None`        = automatico — o MILP manda (default de uma tomada nova);
    ///   `Some(true)`  = cortar, trava explicita do dono ate ele desfazer;
    ///   `Some(false)` = "usar agora" — a `main` converte numa janela temporaria.
    /// Antes da 0002 a coluna era NOT NULL DEFAULT 1: toda tomada recem-provisionada
    /// nascia com `Some(true)` e ficava travada em OFF, descartando o MILP em todo
    /// ciclo. Entregue ao ciclo SUBORDINADO ao P_max (FR-016).
    pub rele_aberto: Option<bool>,
    /// `P_max` canonico da residencia — fonte de verdade do teto eletrico, editavel no
    /// dashboard. Vem junto no mesmo GET (antes exigia uma 2a requisicao).
    pub p_max: Option<f32>,
    /// Chave de coordenacao corrente da casa (64 hex). E assim que uma tomada legitima
    /// reingressa na malha depois de uma rotacao por revogacao, sem re-provisionar
    /// (005/FR-012). `None` contra um backend 004 (campo ausente) => nada muda.
    pub residencia_key: Option<String>,
    /// Hora do servidor (epoch s) no instante da resposta. Fonte de relogio da LAN
    /// para casa SEM internet, onde o SNTP publico nunca sincroniza. `None` contra
    /// um backend anterior (campo ausente) => segue so com SNTP, como antes.
    pub agora_servidor: Option<i64>,
}

#[derive(Deserialize)]
struct RespConfig {
    config_version: i64,
    config: serde_json::Value,
    p_max: i64,
    /// `null` = automatico. Backend anterior a 0002 nunca manda null (era NOT NULL).
    #[serde(default)]
    rele_aberto: Option<bool>,
    /// Ausente em backend anterior a 005 -> `None` (fail-safe: mantem a chave atual).
    #[serde(default)]
    residencia_key: Option<String>,
    /// Ausente em backend que nao serve hora -> `None` (fail-safe: so SNTP).
    #[serde(default)]
    now: Option<i64>,
}

/// GET `/config` da propria tomada (escopo pelo claim; FR-015/016/UC17).
pub fn buscar_desired_state() -> Result<DesiredState> {
    let url = format!("{}/config", backend_url());
    let (status, body) = requisicao(Method::Get, &url, None)?;
    if !(200..=299).contains(&status) {
        return Err(anyhow!("[BACKEND] buscar_desired_state HTTP {status}: {body}"));
    }
    let r: RespConfig = serde_json::from_str(&body).context("parse GET /config")?;

    // `config` chega como objeto; reserializa e parseia no mesmo caminho de 002
    // (`from_str::<Tomada>`), que ja e o trajeto validado no Xtensa.
    let config = match serde_json::from_str::<Tomada>(&r.config.to_string()) {
        Ok(mut c) => {
            c.config_version = r.config_version;
            Some(c)
        }
        Err(e) => {
            log::warn!("[BACKEND] config remota nao parseou: {e}");
            None
        }
    };

    Ok(DesiredState {
        config,
        rele_aberto: r.rele_aberto,
        p_max: Some(r.p_max as f32),
        residencia_key: r.residencia_key.filter(|k| k.len() == 64),
        // Descarta valor implausivel (relogio do servidor errado/zerado): 2020-01-01.
        agora_servidor: r.now.filter(|t| *t > 1_577_836_800),
    })
}

// ============================================================
// Leitura: carga das irmas (fallback multi-no — 003/UC22).
// ============================================================

/// Dado remoto de uma tomada-irma. O backend ja exclui a propria tomada e filtra
/// pela residencia do claim; o frescor e a soma ficam com o firmware (FR-016).
pub struct TomadaRemota {
    pub id: String,
    pub p_medida_atual: f32,
    pub ultimo_uso_ativo_ts: i64,
}

/// GET `/residencias/{id}/tomadas/consumo` — fallback de coordenacao quando o
/// ESP-NOW nao trouxe nenhum peer vivo.
pub fn buscar_consumo_irmas() -> Result<Vec<TomadaRemota>> {
    let rid = residencia_id()?;
    let url = format!("{}/residencias/{rid}/tomadas/consumo", backend_url());
    let (status, body) = requisicao(Method::Get, &url, None)?;
    if !(200..=299).contains(&status) {
        return Err(anyhow!("[BACKEND] buscar_consumo_irmas HTTP {status}: {body}"));
    }
    #[derive(Deserialize)]
    struct Row {
        tomada_id: String,
        #[serde(default)]
        p_medida_atual: f32,
        #[serde(default)]
        ultimo_uso_ativo_ts: i64,
    }
    #[derive(Deserialize)]
    struct Resp {
        tomadas: Vec<Row>,
    }
    let r: Resp = serde_json::from_str(&body).context("parse consumo das irmas")?;
    Ok(r.tomadas
        .into_iter()
        .map(|t| TomadaRemota {
            id: t.tomada_id,
            p_medida_atual: t.p_medida_atual,
            ultimo_uso_ativo_ts: t.ultimo_uso_ativo_ts,
        })
        .collect())
}

// ============================================================
// Helpers de parse/escape.
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

/// Escapa `\`, `"` e caracteres de controle (um `\n` em `detalhes` de erro geraria
/// JSON invalido => lote de eventos preso em retry eterno).
fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}
