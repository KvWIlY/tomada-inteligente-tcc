//! Coordenacao multi-no via ESP-NOW (Principio V; contract `espnow-protocol.md`).
//!
//! Cada no difunde um `Heartbeat` (44 bytes, protocolo v3 autenticado) por ciclo; todos montam o
//! mesmo `SolverInput` e resolvem o MILP de forma replicada/deterministica. Peers vivos
//! viram variaveis de decisao; peers conhecidos porem ausentes viram **carga fixa**
//! (degradacao segura — nunca subestima o consumo => nunca viola o P_max conhecido).
//!
//! NOTA (sem compilador/2 placas aqui): as assinaturas exatas de `EspNow::take`,
//! `register_recv_cb` e `add_peer` variam entre versoes de esp-idf-svc — este e o ponto
//! mais provavel de exigir ajuste fino ao compilar. O `main` trata `iniciar()` como
//! best-effort: se falhar, o no opera sozinho (so a propria tomada).

use anyhow::{anyhow, Result};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use esp_idf_svc::espnow::{EspNow, ReceiveInfo};
use esp_idf_svc::sys;

use crate::comunicacao::provisionamento::agora_epoch;
use tomada_solver::TomadaInput;

type HmacSha256 = Hmac<Sha256>;

/// Versao do protocolo (espnow-protocol.md). v3 = heartbeat AUTENTICADO por HMAC-SHA256
/// com a chave por-residencia (Principio VIII; substitui o CRC publico do v2).
pub const VERSAO: u8 = 3;
/// Bytes de dados do heartbeat (campos 0..28).
const DADOS: usize = 28;
/// Bytes do tag HMAC-SHA256 truncado anexado ao payload.
const TAG: usize = 16;
/// Tamanho total do payload (dados + tag).
pub const TAM: usize = DADOS + TAG;
/// Janela de frescor (s): heartbeat so e aceito se `epoch` estiver dentro de +-este valor
/// do relogio local (anti-replay; relogios SNTP-sincronizados entre os nos).
const FRESCOR_S: i64 = 60;
/// Endereco de broadcast ESP-NOW.
pub const BROADCAST: [u8; 6] = [0xff; 6];

/// Chave HMAC da residencia (32 bytes), setada em `iniciar` a partir das credenciais.
/// Todos os nos da mesma casa compartilham a mesma chave (vinda do provisionamento).
static RES_KEY: OnceLock<[u8; 32]> = OnceLock::new();

/// Heartbeat difundido por cada no a cada ciclo.
#[derive(Debug, Clone, Copy)]
pub struct Heartbeat {
    pub versao: u8,
    pub tomada_id: u8, // indice curto 0..N-1 (informativo; a chave do peer e o MAC)
    pub epoch: u32,
    pub p_medida: f32,
    pub estado: u8,
    pub flags: u8, // bit0=U_i bit1=F_i bit2=rele_manual bit3=A_it(hora)
    pub c_std: f32,
    pub c_on: f32,
    pub pi_hora: f32,
    pub w_i: f32,
}

impl Heartbeat {
    pub fn novo(
        tomada_id: u8,
        epoch: u32,
        p_medida: f32,
        estado: u8,
        u_i: bool,
        f_i: bool,
        rele_manual: bool,
        a_it: bool,
        c_std: f32,
        c_on: f32,
        pi_hora: f32,
        w_i: f32,
    ) -> Self {
        let flags = (u_i as u8) | ((f_i as u8) << 1) | ((rele_manual as u8) << 2) | ((a_it as u8) << 3);
        Heartbeat {
            versao: VERSAO,
            tomada_id,
            epoch,
            p_medida,
            estado,
            flags,
            c_std,
            c_on,
            pi_hora,
            w_i,
        }
    }

    /// Os 28 bytes de dados (sem o tag). f32 via to_le_bytes (operacao de bits — NAO
    /// dispara o bug LLVM Xtensa do serde).
    fn serializar_dados(&self) -> [u8; DADOS] {
        let mut b = [0u8; DADOS];
        b[0] = self.versao;
        b[1] = self.tomada_id;
        b[2..6].copy_from_slice(&self.epoch.to_le_bytes());
        b[6..10].copy_from_slice(&self.p_medida.to_le_bytes());
        b[10] = self.estado;
        b[11] = self.flags;
        b[12..16].copy_from_slice(&self.c_std.to_le_bytes());
        b[16..20].copy_from_slice(&self.c_on.to_le_bytes());
        b[20..24].copy_from_slice(&self.pi_hora.to_le_bytes());
        b[24..28].copy_from_slice(&self.w_i.to_le_bytes());
        b
    }

    /// Payload completo (44 bytes): 28 de dados + HMAC-SHA256(key, dados)[..16].
    pub fn serializar_assinado(&self, key: &[u8; 32]) -> [u8; TAM] {
        let dados = self.serializar_dados();
        let tag = hmac_tag(key, &dados);
        let mut b = [0u8; TAM];
        b[..DADOS].copy_from_slice(&dados);
        b[DADOS..].copy_from_slice(&tag);
        b
    }

    /// Desserializa e AUTENTICA: tamanho, versao, HMAC (tempo ~constante) e frescor do
    /// `epoch`. `None` se qualquer checagem falhar (descarta forjado/replay/de outra casa).
    pub fn desserializar_verificado(b: &[u8], key: &[u8; 32], agora: i64) -> Option<Heartbeat> {
        if b.len() != TAM || b[0] != VERSAO {
            return None;
        }
        // Autenticidade + integridade: HMAC sobre os 28 bytes de dados.
        let esperado = hmac_tag(key, &b[..DADOS]);
        if !ct_eq(&esperado, &b[DADOS..]) {
            return None;
        }
        let f32_at = |o: usize| f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        let hb = Heartbeat {
            versao: b[0],
            tomada_id: b[1],
            epoch: u32::from_le_bytes([b[2], b[3], b[4], b[5]]),
            p_medida: f32_at(6),
            estado: b[10],
            flags: b[11],
            c_std: f32_at(12),
            c_on: f32_at(16),
            pi_hora: f32_at(20),
            w_i: f32_at(24),
        };
        // Anti-replay: epoch dentro da janela de frescor.
        if (agora - hb.epoch as i64).abs() > FRESCOR_S {
            return None;
        }
        Some(hb)
    }

    /// Converte um peer vivo em `TomadaInput` (variavel de decisao do MILP).
    pub fn para_tomada_input(&self) -> TomadaInput {
        TomadaInput {
            c_std: self.c_std as f64,
            c_on: self.c_on as f64,
            u_i: self.flags & 0b0001 != 0,
            f_i: self.flags & 0b0010 != 0,
            pi: self.pi_hora as f64,
            w_i: self.w_i as f64,
            a_it: self.flags & 0b1000 != 0,
        }
    }
}

// ============================================================
// HMAC (autenticacao do heartbeat — protocolo v3)
// ============================================================

/// HMAC-SHA256(key, dados) truncado em `TAG` bytes.
fn hmac_tag(key: &[u8; 32], dados: &[u8]) -> [u8; TAG] {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC aceita qualquer tamanho de chave");
    mac.update(dados);
    let full = mac.finalize().into_bytes(); // 32 bytes
    let mut t = [0u8; TAG];
    t.copy_from_slice(&full[..TAG]);
    t
}

/// Comparacao em tempo ~constante (sem early-return no 1o byte diferente).
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

// ============================================================
// Tabela de peers (RAM), preenchida pelo callback de recepcao.
// ============================================================

fn peers() -> &'static Mutex<HashMap<[u8; 6], (Heartbeat, i64)>> {
    static P: OnceLock<Mutex<HashMap<[u8; 6], (Heartbeat, i64)>>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Inicializa o ESP-NOW: guarda a chave da residencia, registra o callback de recepcao
/// (que AUTENTICA cada heartbeat) e o peer de broadcast. Requer o Wi-Fi ja iniciado.
/// `residencia_key`: chave HMAC compartilhada da casa (do provisionamento).
pub fn iniciar(residencia_key: [u8; 32]) -> Result<EspNow<'static>> {
    let _ = RES_KEY.set(residencia_key);
    let espnow = EspNow::take().map_err(|e| anyhow!("EspNow::take falhou: {e:?}"))?;

    // Callback de recepcao: AUTENTICA (HMAC + frescor) e guarda por MAC do remetente.
    // Heartbeats forjados, de replay ou de outra residencia (HMAC nao bate) sao descartados.
    // esp-idf-svc 0.52: assinatura `FnMut(&ReceiveInfo, &[u8])`; `src_addr` e `&[u8;6]`.
    espnow
        .register_recv_cb(|info: &ReceiveInfo, data: &[u8]| {
            let Some(key) = RES_KEY.get() else { return };
            let agora = agora_epoch();
            if let Some(hb) = Heartbeat::desserializar_verificado(data, key, agora) {
                peers().lock().unwrap().insert(*info.src_addr, (hb, agora));
            }
        })
        .map_err(|e| anyhow!("register_recv_cb falhou: {e:?}"))?;

    // Peer de broadcast.
    let mut peer: sys::esp_now_peer_info_t = unsafe { core::mem::zeroed() };
    peer.peer_addr = BROADCAST;
    peer.channel = 0; // canal atual do Wi-Fi
    peer.ifidx = sys::wifi_interface_t_WIFI_IF_STA;
    peer.encrypt = false;
    espnow
        .add_peer(peer)
        .map_err(|e| anyhow!("add_peer(broadcast) falhou: {e:?}"))?;

    log::info!("[ESPNOW] iniciado (broadcast)");
    Ok(espnow)
}

/// Difunde o heartbeat ASSINADO por broadcast (best-effort; stateless/idempotente).
pub fn difundir(espnow: &EspNow<'static>, hb: &Heartbeat) -> Result<()> {
    let key = RES_KEY
        .get()
        .ok_or_else(|| anyhow!("[ESPNOW] sem residencia_key (iniciar nao chamado)"))?;
    let payload = hb.serializar_assinado(key);
    espnow
        .send(BROADCAST, &payload)
        .map_err(|e| anyhow!("[ESPNOW] send falhou: {e:?}"))
}

/// Monta a coordenacao para o ciclo: `(peers_vivos com MAC, carga_fixa W)`.
///   - peer com heartbeat dentro de `ttl_s` => variavel de decisao (`(MAC, TomadaInput)`).
///   - peer conhecido porem ausente (stale) => carga fixa = ultimo `p_medida` conhecido.
///
/// O MAC acompanha cada peer porque a ordenacao canonica (self + peers por MAC) acontece
/// no ciclo, garantindo entrada IDENTICA entre nos (espnow-protocol.md §"Regras de
/// decisao coordenada" item 4 + §"Requisito de consistencia").
pub fn montar_coordenacao(agora: i64, ttl_s: i64) -> (Vec<([u8; 6], TomadaInput)>, f32) {
    let tabela = peers().lock().unwrap();
    let mut vivos = Vec::new();
    let mut carga_fixa = 0.0f32;
    for (mac, (hb, recebido_em)) in tabela.iter() {
        if agora - recebido_em <= ttl_s {
            vivos.push((*mac, hb.para_tomada_input()));
        } else {
            // Degradacao segura: ultimo p_medida conhecido como carga fixa.
            carga_fixa += hb.p_medida;
        }
    }
    (vivos, carga_fixa)
}

/// Remove peers nao vistos ha muito tempo (limpeza opcional de RAM).
pub fn esquecer_antigos(agora: i64, max_idade_s: i64) {
    peers()
        .lock()
        .unwrap()
        .retain(|_, (_, recebido_em)| agora - *recebido_em <= max_idade_s);
}
