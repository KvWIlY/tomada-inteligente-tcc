//! Medicoes (`Medicao`) — buffer em RAM + baldes por dia em LittleFS.
//!
//! Cadencia 1/min (FR-016). Para proteger a flash (Principio III) NAO gravamos por
//! minuto: acumulamos em RAM e damos `flush` em lote. A persistencia usa **baldes por
//! dia** (`/lfs/med_<dia>.txt`, 1 JSON por linha) para que a purga (retencao.rs) seja a
//! remocao de um arquivo inteiro — barato e sem reescrever grandes arquivos (research §6).
//!
//! Idempotencia/sync: a logica pura (selecao de pendentes, montagem do corpo) vive em
//! `crate::solver::sync` (host-testada). Aqui convertendo `Medicao` <-> `SyncRecord` e
//! cuidando do I/O. `confirmado=true` so apos 2xx, consolidado em UMA reescrita por balde.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::sync::{Mutex, OnceLock};

use crate::solver::relogio::{self, Carimbavel};
use crate::solver::retencao::Balde;
use crate::solver::sync::SyncRecord;
use crate::solver::Estado;

use crate::persistencia::littlefs;

/// Segundos por dia (tamanho do balde).
pub const DIA_S: i64 = 86_400;

/// Um registro de medicao. `confirmado` e estado LOCAL (nao vai no corpo do POST).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Medicao {
    pub tomada_id: String,
    /// Epoch real, ou `relogio::SEM_HORA` (0) enquanto nao houve sincronizacao —
    /// corrigido por deslocamento quando a hora chega (FR-001a).
    pub timestamp: i64,
    /// Potencia **ATIVA** (W) — media do produto instantaneo v*i, nao `Vrms*Irms`.
    pub p_medida: f32,
    /// Fator de potencia `P/S` em [0, 1] da mesma janela. `#[serde(default)]` => 0.0
    /// nos baldes gravados por firmware anterior a 2026-08-24, que nao tinham o campo:
    /// sem o default, `carregar_pendentes` rejeitaria linha a linha e o buffer antigo
    /// nunca sincronizaria.
    #[serde(default)]
    pub fator_potencia: f32,
    pub estado: Estado,
    pub rele_aberto: bool,
    #[serde(default)]
    pub confirmado: bool,
    /// Segundos desde o boot no momento do registro. E o que torna a correcao possivel:
    /// sem ele, uma medicao feita antes da hora certa seria indatavel (005/D3).
    #[serde(default)]
    pub uptime_s: i64,
}

impl Medicao {
    pub fn novo(
        tomada_id: &str,
        timestamp: i64,
        p_medida: f32,
        fator_potencia: f32,
        estado: Estado,
        rele_aberto: bool,
    ) -> Self {
        Medicao {
            tomada_id: tomada_id.to_string(),
            timestamp,
            p_medida,
            fator_potencia,
            estado,
            rele_aberto,
            confirmado: false,
            uptime_s: crate::comunicacao::sntp::uptime_s(),
        }
    }

    pub fn para_sync_record(&self) -> SyncRecord {
        SyncRecord {
            tomada_id: self.tomada_id.clone(),
            timestamp: self.timestamp,
            p_medida: self.p_medida as f64,
            fator_potencia: self.fator_potencia as f64,
            estado: self.estado,
            rele_aberto: self.rele_aberto,
            confirmado: self.confirmado,
        }
    }
}

/// Permite ao `crate::solver::relogio` corrigir os carimbos sem conhecer `Medicao`.
impl Carimbavel for Medicao {
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
        // Falha aqui e quase sempre disco cheio; nomear a causa evita que o erro
        // apareca la longe, no caminho de sync (era o sintoma da divida D-01).
        f.write_all(linhas.as_bytes())
            .with_context(|| format!("gravando balde {path} (disco cheio?)"))?;
        f.flush().with_context(|| format!("flush do balde {path} (disco cheio?)"))?;
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

/// Inventario dos baldes em disco para a purga por espaco: tamanho em bytes e se o
/// balde inteiro ja foi confirmado pelo servidor. Le cada balde UMA vez.
/// Balde vazio/ilegivel conta como confirmado — nao ha dado a perder nele.
pub(crate) fn inventario() -> Result<Vec<Balde>> {
    let mut out = Vec::new();
    for dia in dias_em_disco()? {
        let bytes = std::fs::metadata(caminho_balde(dia)).map(|m| m.len()).unwrap_or(0);
        let todos_confirmados = ler_balde(dia)?.iter().all(|m| m.confirmado);
        out.push(Balde { dia, bytes, todos_confirmados });
    }
    Ok(out)
}

/// Remove um balde inteiro. O(1): um `remove`, sem reescrever nada (Principio III).
pub(crate) fn remover_balde(dia: i64) -> Result<()> {
    let path = caminho_balde(dia);
    std::fs::remove_file(&path).with_context(|| format!("removendo {path}"))
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
/// `crate::solver::sync::selecionar_pendentes` (host-testada) sobre os `SyncRecord`.
///
/// Registros SEM HORA (`timestamp == SEM_HORA`) nunca sobem: ou sao corrigidos por
/// `corrigir_carimbos` quando a hora chega, ou — se sobraram de um boot anterior que
/// nunca teve hora — sao incorrigiveis e ficam de fora (FR-001a, excecao unica).
pub fn carregar_pendentes() -> Result<Vec<Medicao>> {
    flush_para_disco()?;
    let mut pend = Vec::new();
    let mut sem_hora = 0usize;
    for dia in dias_em_disco()? {
        for m in ler_balde(dia)? {
            if m.confirmado {
                continue;
            }
            if m.timestamp == relogio::SEM_HORA {
                sem_hora += 1;
                continue;
            }
            pend.push(m);
        }
    }
    if sem_hora > 0 {
        log::warn!("[MED] {sem_hora} medicoes sem hora certa ficaram de fora do sync");
    }
    Ok(pend)
}

/// Aplica o deslocamento do 1o sync SNTP a tudo que foi medido sem hora (FR-001a).
///
/// Corrige o buffer de RAM e reescreve os baldes afetados UMA vez (Principio III). Os
/// registros do buffer sao os do boot atual — corrigiveis por definicao; em disco,
/// corrige os que tem `uptime_s` e descarta os incorrigiveis de boots anteriores.
/// Devolve `(corrigidos, descartados)` para o evento de observabilidade.
pub fn corrigir_carimbos(offset: i64) -> Result<(usize, usize)> {
    let mut corrigidos = 0usize;
    let mut descartados = 0usize;

    {
        let mut buf = buffer().lock().unwrap();
        let c = relogio::corrigir(&mut buf[..], offset);
        corrigidos += c.corrigidos;
    }

    for dia in dias_em_disco()? {
        let mut regs = ler_balde(dia)?;
        if !regs.iter().any(|m| m.timestamp == relogio::SEM_HORA) {
            continue; // balde intacto: nao reescreve
        }
        // Sem `uptime_s` gravado (registro de boot anterior a este campo, ou de um
        // boot que nunca teve hora) nao ha como datar: descarta.
        let antes = regs.len();
        regs.retain(|m| m.timestamp != relogio::SEM_HORA || m.uptime_s > 0);
        descartados += antes - regs.len();

        let c = relogio::corrigir(&mut regs[..], offset);
        corrigidos += c.corrigidos;
        reescrever_balde(dia, &regs)?;
    }

    if corrigidos > 0 || descartados > 0 {
        log::info!("[MED] carimbos: {corrigidos} corrigidos, {descartados} descartados");
    }
    Ok((corrigidos, descartados))
}

/// Descarta, na carga do boot, medicoes sem hora vindas de boots anteriores — elas
/// nunca poderao ser datadas (FR-001a). Devolve quantas foram removidas.
pub fn descartar_sem_hora_de_boot_anterior() -> Result<usize> {
    let mut total = 0usize;
    for dia in dias_em_disco()? {
        let mut regs = ler_balde(dia)?;
        let antes = regs.len();
        regs.retain(|m| m.timestamp != relogio::SEM_HORA);
        let n = antes - regs.len();
        if n > 0 {
            total += n;
            reescrever_balde(dia, &regs)?;
        }
    }
    if total > 0 {
        log::warn!("[MED] {total} medicoes de boot anterior sem hora certa descartadas");
    }
    Ok(total)
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

/// Reescreve um balde inteiro (ou o remove, se ficou vazio).
fn reescrever_balde(dia: i64, regs: &[Medicao]) -> Result<()> {
    let path = caminho_balde(dia);
    if regs.is_empty() {
        let _ = std::fs::remove_file(&path);
        return Ok(());
    }
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
