//! Pareamento local entre tomadas (feature 005, US3 —
//! `contracts/pareamento-espnow.md` parte B).
//!
//! Resolve o caso que a nuvem nao resolve: uma casa SEM internet e SEM servidor onde o
//! dono acabou de comprar a segunda tomada. A chave da residencia (que autentica os
//! heartbeats) precisa chegar na tomada nova sem passar por servidor nenhum.
//!
//! Dupla prova de posse (Clarification Q1):
//!   1. **botao fisico** na tomada que ja funciona (abre janela de 60 s), e
//!   2. **PIN de 8 caracteres** dela, digitado no Portal AP da tomada nova.
//!
//! O handshake (SPAKE2 -> HKDF -> ChaCha20-Poly1305) vive em `tomada_solver::pareamento`
//! (puro, testado no host). Aqui so ha radio, temporizacao e persistencia.

use anyhow::{anyhow, Result};
use esp_idf_svc::espnow::{EspNow, ReceiveInfo};
use esp_idf_svc::sys;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use crate::comunicacao::sntp::uptime_s;
use tomada_solver::pareamento::{
    self, ErroPareamento, Janela, TAM_CHAVE, TAM_NONCE, DURACAO_JANELA_S,
};

// Tipos de frame (byte 0). Fora da faixa do heartbeat (que comeca com `versao=5`).
const PAIR_BEACON: u8 = 0xB0;
const PAIR_REQ: u8 = 0xB1;
const PAIR_RESP: u8 = 0xB2;
const PAIR_KEY: u8 = 0xB3;
const PAIR_ACK: u8 = 0xB4;

/// Cadencia do anuncio da tomada existente.
const INTERVALO_BEACON_MS: u64 = 500;
/// Teto da varredura de canais na tomada nova.
const TIMEOUT_DESCOBERTA_S: i64 = 30;
/// Canais Wi-Fi 2,4 GHz varridos a procura do beacon.
const CANAIS: [u8; 13] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13];

/// Motivo da falha, exibido no portal e registrado como evento (Principio VII).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FalhaPareamento {
    /// Nenhuma tomada anunciando: janela fechada ou fora de alcance.
    SemBeacon,
    /// A tag AEAD nao validou — PIN digitado nao confere.
    PinIncorreto,
    /// A outra tomada parou de responder no meio do handshake.
    Timeout,
    /// Radio indisponivel.
    Radio,
}

impl FalhaPareamento {
    pub fn as_str(self) -> &'static str {
        match self {
            FalhaPareamento::SemBeacon => "sem_beacon",
            FalhaPareamento::PinIncorreto => "pin_incorreto",
            FalhaPareamento::Timeout => "timeout",
            FalhaPareamento::Radio => "radio",
        }
    }

    pub fn mensagem(self) -> &'static str {
        match self {
            FalhaPareamento::SemBeacon => {
                "Nenhuma tomada em modo pareamento por perto. Aperte o botao da outra tomada e tente de novo."
            }
            FalhaPareamento::PinIncorreto => "PIN incorreto.",
            FalhaPareamento::Timeout => "A outra tomada parou de responder.",
            FalhaPareamento::Radio => "Radio indisponivel.",
        }
    }
}

impl From<ErroPareamento> for FalhaPareamento {
    fn from(e: ErroPareamento) -> Self {
        match e {
            ErroPareamento::PinIncorreto => FalhaPareamento::PinIncorreto,
            _ => FalhaPareamento::Timeout,
        }
    }
}

// ============================================================
// Caixa de entrada dos frames de pareamento (alimentada pelo callback do ESP-NOW)
// ============================================================

struct Entrada {
    origem: [u8; 6],
    dados: Vec<u8>,
}

fn canal() -> &'static (Mutex<Sender<Entrada>>, Mutex<Receiver<Entrada>>) {
    static C: OnceLock<(Mutex<Sender<Entrada>>, Mutex<Receiver<Entrada>>)> = OnceLock::new();
    C.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        (Mutex::new(tx), Mutex::new(rx))
    })
}

/// Encaminha um frame de pareamento vindo do callback global do ESP-NOW.
/// `true` se o frame era de pareamento (o chamador nao deve trata-lo como heartbeat).
pub fn talvez_encaminhar(info: &ReceiveInfo, data: &[u8]) -> bool {
    if data.is_empty() || !(PAIR_BEACON..=PAIR_ACK).contains(&data[0]) {
        return false;
    }
    let tx = canal().0.lock().unwrap();
    let _ = tx.send(Entrada {
        origem: *info.src_addr,
        dados: data.to_vec(),
    });
    true
}

fn receber(timeout: Duration) -> Option<Entrada> {
    canal().1.lock().unwrap().recv_timeout(timeout).ok()
}

fn drenar() {
    while canal().1.lock().unwrap().try_recv().is_ok() {}
}

fn enviar(espnow: &EspNow<'static>, destino: &[u8; 6], tipo: u8, corpo: &[u8]) -> Result<()> {
    let mut payload = Vec::with_capacity(1 + corpo.len());
    payload.push(tipo);
    payload.extend_from_slice(corpo);
    garantir_peer(espnow, destino)?;
    espnow
        .send(*destino, &payload)
        .map_err(|e| anyhow!("[PAIR] send falhou: {e:?}"))
}

fn garantir_peer(espnow: &EspNow<'static>, mac: &[u8; 6]) -> Result<()> {
    if espnow.peer_exists(*mac).unwrap_or(false) {
        return Ok(());
    }
    let mut peer: sys::esp_now_peer_info_t = unsafe { core::mem::zeroed() };
    peer.peer_addr = *mac;
    peer.channel = 0;
    peer.ifidx = sys::wifi_interface_t_WIFI_IF_STA;
    peer.encrypt = false;
    espnow
        .add_peer(peer)
        .map_err(|e| anyhow!("[PAIR] add_peer falhou: {e:?}"))
}

// ============================================================
// Lado EXISTENTE (E): botao abre a janela e entrega a chave
// ============================================================

/// Janela de pareamento ativa (aberta pelo botao). `None` = fechada.
static JANELA: Mutex<Option<Janela>> = Mutex::new(None);

/// Abre (ou reabre) a janela de 60 s — chamada pela task do botao (FR-008).
pub fn abrir_janela() {
    *JANELA.lock().unwrap() = Some(Janela::abrir(uptime_s()));
    log::warn!(
        "[PAIR] janela de pareamento ABERTA por {DURACAO_JANELA_S}s -- digite o PIN desta tomada na tomada nova"
    );
}

pub fn janela_aberta() -> bool {
    JANELA
        .lock()
        .unwrap()
        .map(|j| j.aberta(uptime_s()))
        .unwrap_or(false)
}

/// Atende o pareamento enquanto a janela estiver aberta. Chamada uma vez por ciclo
/// pelo `main` (nao bloqueia quando a janela esta fechada).
pub fn atender(
    espnow: &EspNow<'static>,
    pin_proprio: &str,
    proprio_mac: &[u8; 6],
    residencia_key: &[u8; TAM_CHAVE],
) {
    if !janela_aberta() {
        return;
    }
    // Anuncia e escuta por um ciclo.
    let _ = enviar(espnow, &super::espnow::BROADCAST, PAIR_BEACON, &[1]);
    std::thread::sleep(Duration::from_millis(INTERVALO_BEACON_MS));

    let Some(e) = receber(Duration::from_millis(INTERVALO_BEACON_MS)) else {
        return;
    };
    if e.dados.first() != Some(&PAIR_REQ) {
        return;
    }

    match entregar_chave(espnow, pin_proprio, proprio_mac, residencia_key, &e) {
        Ok(()) => {
            log::warn!("[PAIR] chave entregue a {:02X?} -- janela fechada", e.origem);
            if let Some(j) = JANELA.lock().unwrap().as_mut() {
                j.concluir();
            }
        }
        Err(f) => {
            log::warn!("[PAIR] tentativa falhou ({}) de {:02X?}", f.as_str(), e.origem);
            // Falha de autenticacao consome uma das 3 tentativas (FR-009).
            if let Some(j) = JANELA.lock().unwrap().as_mut() {
                j.falhou();
                if !j.aberta(uptime_s()) {
                    log::warn!("[PAIR] janela FECHADA (tentativas esgotadas)");
                }
            }
        }
    }
}

fn entregar_chave(
    espnow: &EspNow<'static>,
    pin: &str,
    proprio_mac: &[u8; 6],
    residencia_key: &[u8; TAM_CHAVE],
    req: &Entrada,
) -> Result<(), FalhaPareamento> {
    let (sessao, resp) =
        pareamento::responder_existente(pin, &req.origem, proprio_mac, &req.dados[1..])
            .map_err(FalhaPareamento::from)?;
    enviar(espnow, &req.origem, PAIR_RESP, &resp).map_err(|_| FalhaPareamento::Radio)?;

    let mut nonce = [0u8; TAM_NONCE];
    preencher_aleatorio(&mut nonce);
    let cifrado = pareamento::embrulhar_chave(&sessao, &nonce, residencia_key);

    let mut corpo = Vec::with_capacity(TAM_NONCE + cifrado.len());
    corpo.extend_from_slice(&nonce);
    corpo.extend_from_slice(&cifrado);
    enviar(espnow, &req.origem, PAIR_KEY, &corpo).map_err(|_| FalhaPareamento::Radio)?;

    // Confirmacao: prova que a nova decifrou com o PIN certo.
    let ack = receber(Duration::from_secs(5)).ok_or(FalhaPareamento::Timeout)?;
    if ack.dados.first() != Some(&PAIR_ACK) {
        return Err(FalhaPareamento::Timeout);
    }
    pareamento::verificar_ack(residencia_key, &req.origem, &ack.dados[1..])
        .map_err(|_| FalhaPareamento::PinIncorreto)
}

// ============================================================
// Lado NOVA (N): varre canais, faz o handshake e recebe a chave
// ============================================================

/// Executa o pareamento a partir do PIN digitado no portal. Devolve a chave da casa.
pub fn parear_como_nova(
    espnow: &EspNow<'static>,
    pin_digitado: &str,
    proprio_mac: &[u8; 6],
) -> Result<[u8; TAM_CHAVE], FalhaPareamento> {
    let mac_existente = descobrir(espnow)?;
    log::info!("[PAIR] tomada encontrada: {mac_existente:02X?}");

    let (estado, req) = pareamento::iniciar_nova(pin_digitado, proprio_mac, &mac_existente);
    enviar(espnow, &mac_existente, PAIR_REQ, &req).map_err(|_| FalhaPareamento::Radio)?;

    let resp = esperar(PAIR_RESP, 5)?;
    let chave_sessao =
        pareamento::concluir_nova(estado, &resp.dados[1..]).map_err(FalhaPareamento::from)?;

    let key_frame = esperar(PAIR_KEY, 5)?;
    if key_frame.dados.len() < 1 + TAM_NONCE {
        return Err(FalhaPareamento::Timeout);
    }
    let mut nonce = [0u8; TAM_NONCE];
    nonce.copy_from_slice(&key_frame.dados[1..1 + TAM_NONCE]);

    // Aqui e onde o PIN errado e detectado: a tag AEAD nao valida.
    let chave = pareamento::desembrulhar_chave(
        &chave_sessao,
        &nonce,
        &key_frame.dados[1 + TAM_NONCE..],
    )
    .map_err(FalhaPareamento::from)?;

    let ack = pareamento::ack(&chave, proprio_mac);
    enviar(espnow, &mac_existente, PAIR_ACK, &ack).map_err(|_| FalhaPareamento::Radio)?;
    Ok(chave)
}

/// Varre os canais a procura do `PAIR_BEACON`. Multiplos anunciantes: tenta o
/// primeiro; se o handshake falhar, o dono repete (o PIN so fecha com a tomada certa).
fn descobrir(_espnow: &EspNow<'static>) -> Result<[u8; 6], FalhaPareamento> {
    drenar();
    let inicio = uptime_s();
    loop {
        for canal in CANAIS {
            unsafe {
                sys::esp_wifi_set_channel(canal, sys::wifi_second_chan_t_WIFI_SECOND_CHAN_NONE);
            }
            if let Some(e) = receber(Duration::from_millis(300)) {
                if e.dados.first() == Some(&PAIR_BEACON) {
                    log::info!("[PAIR] beacon no canal {canal}");
                    return Ok(e.origem);
                }
            }
            if uptime_s() - inicio > TIMEOUT_DESCOBERTA_S {
                return Err(FalhaPareamento::SemBeacon);
            }
        }
    }
}

fn esperar(tipo: u8, segundos: u64) -> Result<Entrada, FalhaPareamento> {
    let limite = uptime_s() + segundos as i64;
    while uptime_s() <= limite {
        if let Some(e) = receber(Duration::from_millis(500)) {
            if e.dados.first() == Some(&tipo) {
                return Ok(e);
            }
        }
    }
    Err(FalhaPareamento::Timeout)
}

fn preencher_aleatorio(buf: &mut [u8]) {
    for chunk in buf.chunks_mut(4) {
        let r = unsafe { sys::esp_random() }.to_le_bytes();
        for (i, b) in chunk.iter_mut().enumerate() {
            *b = r[i];
        }
    }
}
