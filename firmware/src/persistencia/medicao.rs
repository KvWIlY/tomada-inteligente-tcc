//! Medicoes (`Medicao`) — buffer em RAM + baldes por dia em LittleFS.
//!
//! Cadencia 1/min (FR-016). Para proteger a flash (Principio III) NAO gravamos por
//! minuto: acumulamos em RAM e damos `flush` em lote. A persistencia usa **baldes por
//! dia** (`/lfs/med_<dia>.txt`, 1 JSON por linha) para que a purga (retencao.rs) seja a
//! remocao de um arquivo inteiro — barato e sem reescrever grandes arquivos (research §6).
//!
//! Idempotencia/sync: a logica pura (selecao de pendentes, montagem do corpo) vive em
//! `tomada_solver::sync` (host-testada). Aqui convertendo `Medicao` <-> `SyncRecord` e
//! cuidando do I/O. `confirmado=true` so apos 2xx, consolidado em UMA reescrita por balde.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::sync::{Mutex, OnceLock};

use tomada_solver::sync::SyncRecord;
use tomada_solver::Estado;

use crate::persistencia::littlefs;

/// Segundos por dia (tamanho do balde).
pub const DIA_S: i64 = 86_400;

/// Um registro de medicao. `confirmado` e estado LOCAL (nao vai no corpo do POST).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Medicao {
    pub tomada_id: String,
    pub timestamp: i64,
    pub p_medida: f32,
    pub estado: Estado,
    pub rele_aberto: bool,
    #[serde(default)]
    pub confirmado: bool,
}

impl Medicao {
    pub fn novo(tomada_id: &str, timestamp: i64, p_medida: f32, estado: Estado, rele_aberto: bool) -> Self {
        Medicao { tomada_id: tomada_id.to_string(), timestamp, p_medida, estado, rele_aberto, confirmado: false }
    }

    pub fn para_sync_record(&self) -> SyncRecord {
        SyncRecord {
            tomada_id: self.tomada_id.clone(),
            timestamp: self.timestamp,
            p_medida: self.p_medida as f64,
            estado: self.estado,
            rele_aberto: self.rele_aberto,
            confirmado: self.confirmado,
        }
    }
}

/// Buffer em RAM dos registros ainda nao gravados em disco.
fn buffer() -> &'static Mutex<Vec<Medicao>> {
    static B: OnceLock<Mutex<Vec<Medicao>>> = OnceLock::new();
    B.get_or_init(|| Mutex::new(Vec::new()))
}

/// Registra uma medicao no buffer de RAM (sem tocar a flash).
pub fn registrar(m: Medicao) {
    buffer().lock().unwrap().push(m);
}

/// Apaga TODOS os baldes de medicao em disco + o buffer de RAM. Usado no reset de
/// identidade (build novo/reprovisionamento): medicoes gravadas com o `tomada_id` antigo
/// nao passam no `with check` da RLS sob o JWT novo (403) e, como nunca confirmam,
/// travariam o sync — descarta-las evita esse deadlock.
pub fn limpar_tudo() {
    buffer().lock().unwrap().clear();
    if let Ok(dias) = dias_em_disco() {
        for dia in dias {
            let _ = std::fs::remove_file(caminho_balde(dia));
        }
    }
}

pub(crate) fn caminho_balde(dia: i64) -> String {
    format!("{}/med_{}.txt", littlefs::base(), dia)
}

/// Grava o buffer de RAM nos baldes do dia (append) e limpa a RAM. Escrita EM LOTE.
/// Registros de dias diferentes vao para baldes diferentes (raro; corte de meia-noite).
pub fn flush_para_disco() -> Result<usize> {
    let mut buf = buffer().lock().unwrap();
    if buf.is_empty() {
        return Ok(0);
    }
    // Agrupa por dia para abrir cada balde uma vez.
    use std::collections::BTreeMap;
    let mut por_dia: BTreeMap<i64, Vec<&Medicao>> = BTreeMap::new();
    for m in buf.iter() {
        por_dia.entry(m.timestamp / DIA_S).or_default().push(m);
    }
    let mut total = 0usize;
    for (dia, regs) in por_dia {
        let path = caminho_balde(dia);
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("abrindo balde {path}"))?;
        let mut linhas = String::new();
        for m in regs {
            linhas.push_str(&serde_json::to_string(m).context("serializando medicao")?);
            linhas.push('\n');
            total += 1;
        }
        f.write_all(linhas.as_bytes())?;
        f.flush()?;
    }
    buf.clear();
    log::info!("[MED] flush: {total} registros gravados em disco");
    Ok(total)
}

/// Lista os dias (baldes) existentes em disco, crescente.
pub(crate) fn dias_em_disco() -> Result<Vec<i64>> {
    let base = littlefs::base();
    let mut dias = Vec::new();
    for entry in std::fs::read_dir(base).with_context(|| format!("lendo {base}"))? {
        let entry = entry?;
        let nome = entry.file_name().to_string_lossy().to_string();
        if let Some(resto) = nome.strip_prefix("med_") {
            if let Some(num) = resto.strip_suffix(".txt") {
                if let Ok(dia) = num.parse::<i64>() {
                    dias.push(dia);
                }
            }
        }
    }
    dias.sort_unstable();
    Ok(dias)
}

pub(crate) fn ler_balde(dia: i64) -> Result<Vec<Medicao>> {
    let path = caminho_balde(dia);
    let conteudo = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e).with_context(|| format!("lendo balde {path}")),
    };
    let mut regs = Vec::new();
    for linha in conteudo.lines().filter(|l| !l.trim().is_empty()) {
        match serde_json::from_str::<Medicao>(linha) {
            Ok(m) => regs.push(m),
            Err(e) => log::warn!("[MED] linha invalida em {path} ignorada: {e}"),
        }
    }
    Ok(regs)
}

/// Carrega TODOS os registros pendentes (`confirmado=false`) de todos os baldes.
/// Faz flush antes para incluir os de RAM. A selecao/ordenacao/limite finais ficam com
/// `tomada_solver::sync::selecionar_pendentes` (host-testada) sobre os `SyncRecord`.
pub fn carregar_pendentes() -> Result<Vec<Medicao>> {
    flush_para_disco()?;
    let mut pend = Vec::new();
    for dia in dias_em_disco()? {
        for m in ler_balde(dia)? {
            if !m.confirmado {
                pend.push(m);
            }
        }
    }
    Ok(pend)
}

/// Marca como `confirmado=true` os registros cujos `timestamp` constam em `ts_ok`,
/// reescrevendo cada balde afetado UMA unica vez (consolidacao em lote, Principio III).
/// (Cada device e UMA tomada => `timestamp` identifica o registro.)
pub fn marcar_confirmados(ts_ok: &[i64]) -> Result<()> {
    if ts_ok.is_empty() {
        return Ok(());
    }
    use std::collections::HashSet;
    let set: HashSet<i64> = ts_ok.iter().copied().collect();
    for dia in dias_em_disco()? {
        let mut regs = ler_balde(dia)?;
        let mut mudou = false;
        for m in regs.iter_mut() {
            if !m.confirmado && set.contains(&m.timestamp) {
                m.confirmado = true;
                mudou = true;
            }
        }
        if mudou {
            reescrever_balde(dia, &regs)?;
        }
    }
    Ok(())
}

fn reescrever_balde(dia: i64, regs: &[Medicao]) -> Result<()> {
    let path = caminho_balde(dia);
    let mut linhas = String::new();
    for m in regs {
        linhas.push_str(&serde_json::to_string(m).context("serializando medicao")?);
        linhas.push('\n');
    }
    let mut f = std::fs::File::create(&path).with_context(|| format!("reescrevendo {path}"))?;
    f.write_all(linhas.as_bytes())?;
    f.flush()?;
    Ok(())
}
