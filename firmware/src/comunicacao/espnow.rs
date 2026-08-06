//! Coordenacao multi-no via ESP-NOW (Principio V; contrato
//! `specs/005-offline-pareamento-local/contracts/pareamento-espnow.md` parte A).
//!
//! Cada no difunde um `Heartbeat` (48 bytes, protocolo v5 autenticado) por ciclo; todos
//! montam o mesmo `SolverInput` e resolvem o MILP de forma replicada/deterministica.
//! Peers vivos viram variaveis de decisao; peers conhecidos porem ausentes viram **carga
//! fixa** (degradacao segura — nunca subestima o consumo => nunca viola o P_max).
//!
//! v5 (feature 005): o frescor deixou de ser `epoch` +- 30 s (que exigia SNTP nos dois
//! lados) e passou a ser `(boot_id, contador)` por remetente — a coordenacao funciona
//! numa casa sem internet, com relogios completamente diferentes. A chave da casa vive
//! num `Mutex` porque ROTACIONA quando o dono revoga uma tomada.
//!
//! NOTA: as assinaturas exatas de `EspNow::take`, `register_recv_cb` e `add_peer` variam
//! entre versoes de esp-idf-svc — ponto provavel de ajuste fino ao compilar. O `main`
//! trata `iniciar()` como best-effort: se falhar, o no opera sozinho.

use anyhow::{anyhow, Result};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

use esp_idf_svc::espnow::{EspNow, ReceiveInfo};
use esp_idf_svc::sys;

use tomada_solver::replay::{self, EstadoPeer};
use tomada_solver::TomadaInput;

type HmacSha256 = Hmac<Sha256>;

/// Versao do protocolo (005: `contracts/pareamento-espnow.md` parte A).
/// v5 = v4 (HMAC-SHA256 por-residencia cobrindo dados + MAC do remetente) com o
/// `epoch` trocado por `(boot_id, contador)`: o frescor deixa de exigir relogio
/// sincronizado — numa casa sem internet, o v4 fazia os nos REJEITAREM heartbeats
/// legitimos uns dos outros e a coordenacao morria em silencio (FR-010).
pub const VERSAO: u8 = 5;
/// Bytes de dados do heartbeat (campos 0..32).
const DADOS: usize = 32;
/// Bytes do tag HMAC-SHA256 truncado anexado ao payload.
const TAG: usize = 16;
/// Tamanho total do payload (dados + tag) = 48 B.
pub const TAM: usize = DADOS + TAG;
/// Endereco de broadcast ESP-NOW.
pub const BROADCAST: [u8; 6] = [0xff; 6];

/// Chave HMAC da residencia (32 bytes). `Mutex` (e nao `OnceLock`) porque a chave
/// ROTACIONA quando o dono revoga uma tomada no dashboard: as legitimas recebem a nova
/// no `GET /config` e trocam em runtime, sem reflash (FR-012).
static RES_KEY: Mutex<Option<[u8; 32]>> = Mutex::new(None);

/// Identificador deste boot (32 bits aleatorios, nunca 0) e contador de difusoes.
/// Substituem o `epoch` do v4 no anti-replay — vivem em RAM, zero escrita em Flash
/// (Principio III).
static BOOT_ID: OnceLock<u32> = OnceLock::new();
static CONTADOR: AtomicU32 = AtomicU32::new(0);

fn boot_id() -> u32 {
    *BOOT_ID.get_or_init(|| {
        let mut v = unsafe { sys::esp_random() };
        if v == 0 {
            v = 1; // 0 e reservado (`replay::aceitar` recusa)
        }
        v
    })
}

/// Chave ativa da residencia (`None` = coordenacao desabilitada, fail-closed).
pub fn chave_ativa() -> Option<[u8; 32]> {
    *RES_KEY.lock().unwrap()
}

/// Troca a chave da casa em runtime (rotacao apos revogacao — FR-012).
/// Peers autenticados com a chave antiga deixam de validar; a particao temporaria
/// enquanto as irmas nao atualizam e aceita (Clarification Q4).
pub fn definir_chave(nova: [u8; 32]) {
    *RES_KEY.lock().unwrap() = Some(nova);
    peers().lock().unwrap().clear();
    log::warn!("[ESPNOW] chave da residencia trocada -- tabela de peers zerada");
}

/// Heartbeat difundido por cada no a cada ciclo.
#[derive(Debug, Clone, Copy)]
pub struct Heartbeat {
    pub versao: u8,
    pub tomada_id: u8, // indice curto 0..N-1 (informativo; a chave do peer e o MAC)
    /// Identificador do boot do REMETENTE (aleatorio por boot, nunca 0).
    pub boot_id: u32,
    /// Contador crescente dentro deste boot (anti-replay sem relogio).
    pub contador: u32,
    pub p_medida: f32,
    pub estado: u8,
    pub flags: u8, // bit0=U_i bit1=F_i bit2=rele_manual bit3=A_it(hora)
    pub c_std: f32,
    pub c_on: f32,
    pub pi_hora: f32,
    pub w_i: f32,
}

impl Heartbeat {
    /// Monta o heartbeat deste ciclo. `boot_id`/`contador` sao deste no (o contador
    /// avanca a cada chamada — um quadro, um numero).
    #[allow(clippy::too_many_arguments)]
    pub fn novo(
        tomada_id: u8,
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
        let contador = {
            let atual = CONTADOR.load(Ordering::Relaxed);
            let prox = tomada_solver::replay::proximo_contador(atual);
            CONTADOR.store(prox, Ordering::Relaxed);
            prox
        };
        Heartbeat {
            versao: VERSAO,
            tomada_id,
            boot_id: boot_id(),
            contador,
            p_medida,
            estado,
            flags,
            c_std,
            c_on,
            pi_hora,
            w_i,
        }
    }

    /// Os 32 bytes de dados (sem o tag), conforme a tabela de offsets do contrato
    /// `pareamento-espnow.md` parte A. f32 via to_le_bytes (operacao de bits — NAO
    /// dispara o bug LLVM Xtensa do serde).
    fn serializar_dados(&self) -> [u8; DADOS] {
        let mut b = [0u8; DADOS];
        b[0] = self.versao;
        b[1] = self.tomada_id;
        b[2..6].copy_from_slice(&self.boot_id.to_le_bytes());
        b[6..10].copy_from_slice(&self.contador.to_le_bytes());
        b[10..14].copy_from_slice(&self.p_medida.to_le_bytes());
        b[14] = self.estado;
        b[15] = self.flags;
        b[16..20].copy_from_slice(&self.c_std.to_le_bytes());
        b[20..24].copy_from_slice(&self.c_on.to_le_bytes());
        b[24..28].copy_from_slice(&self.pi_hora.to_le_bytes());
        b[28..32].copy_from_slice(&self.w_i.to_le_bytes());
        b
    }

    /// Payload completo (48 bytes): 32 de dados + HMAC-SHA256(key, dados || MAC)[..16].
    /// `mac` = MAC do REMETENTE (nao trafega no payload — o receptor o ve no frame).
    pub fn serializar_assinado(&self, key: &[u8; 32], mac: &[u8; 6]) -> [u8; TAM] {
        let dados = self.serializar_dados();
        let tag = hmac_tag(key, &dados, mac);
        let mut b = [0u8; TAM];
        b[..DADOS].copy_from_slice(&dados);
        b[DADOS..].copy_from_slice(&tag);
        b
    }

    /// Desserializa e AUTENTICA: tamanho, versao e HMAC (tempo ~constante, cobrindo o
    /// `src_mac` do frame). `None` se qualquer checagem falhar (forjado/MAC spoofado/de
    /// outra casa). O anti-replay NAO acontece aqui: e decidido por remetente em
    /// `tomada_solver::replay::aceitar`, no callback de recepcao — e por isso nao
    /// depende mais de relogio (FR-010).
    pub fn desserializar_verificado(
        b: &[u8],
        key: &[u8; 32],
        src_mac: &[u8; 6],
    ) -> Option<Heartbeat> {
        if b.len() != TAM || b[0] != VERSAO {
            return None;
        }
        // Autenticidade + integridade: HMAC sobre os 32 bytes de dados + MAC de origem.
        let esperado = hmac_tag(key, &b[..DADOS], src_mac);
        if !ct_eq(&esperado, &b[DADOS..]) {
            return None;
        }
        let f32_at = |o: usize| f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        Some(Heartbeat {
            versao: b[0],
            tomada_id: b[1],
            boot_id: u32::from_le_bytes([b[2], b[3], b[4], b[5]]),
            contador: u32::from_le_bytes([b[6], b[7], b[8], b[9]]),
            p_medida: f32_at(10),
            estado: b[14],
            flags: b[15],
            c_std: f32_at(16),
            c_on: f32_at(20),
            pi_hora: f32_at(24),
            w_i: f32_at(28),
        })
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

/// HMAC-SHA256(key, dados || mac_remetente) truncado em `TAG` bytes. Cobrir o MAC
/// amarra o payload a UM remetente (v4): replay com MAC spoofado nao autentica.
fn hmac_tag(key: &[u8; 32], dados: &[u8], mac_remetente: &[u8; 6]) -> [u8; TAG] {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC aceita qualquer tamanho de chave");
    mac.update(dados);
    mac.update(mac_remetente);
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

/// Um peer conhecido: ultimo heartbeat aceito, quando chegou (UPTIME local — nunca
/// SNTP) e o estado de anti-replay do remetente.
struct Peer {
    hb: Heartbeat,
    recebido_em: i64,
    replay: EstadoPeer,
}

fn peers() -> &'static Mutex<HashMap<[u8; 6], Peer>> {
    static P: OnceLock<Mutex<HashMap<[u8; 6], Peer>>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Inicializa o ESP-NOW: guarda a chave da residencia, registra o callback de recepcao
/// (que AUTENTICA cada heartbeat) e o peer de broadcast. Requer o Wi-Fi ja iniciado.
/// `residencia_key`: chave HMAC compartilhada da casa (provisionamento, pareamento
/// local ou rotacao).
pub fn iniciar(residencia_key: [u8; 32]) -> Result<EspNow<'static>> {
    *RES_KEY.lock().unwrap() = Some(residencia_key);
    let espnow = EspNow::take().map_err(|e| anyhow!("EspNow::take falhou: {e:?}"))?;

    // Callback de recepcao: AUTENTICA (HMAC) e aplica o anti-replay por remetente.
    // Forjados, de outra residencia (HMAC nao bate) ou reapresentados (contador nao
    // cresceu no mesmo boot_id) sao descartados — sem depender de relogio (FR-010).
    espnow
        .register_recv_cb(|info: &ReceiveInfo, data: &[u8]| {
            // Frames de pareamento (0xB0..0xB4) vao para o outro modulo: sao a UNICA
            // conversa ESP-NOW que acontece SEM a chave da casa — e justamente para
            // obte-la (FR-008).
            if crate::comunicacao::pareamento::talvez_encaminhar(info, data) {
                return;
            }
            let Some(key) = chave_ativa() else { return };
            let Some(hb) = Heartbeat::desserializar_verificado(data, &key, info.src_addr) else {
                return;
            };
            let mut tabela = peers().lock().unwrap();
            let anterior = tabela.get(info.src_addr).map(|p| p.replay);
            let (veredito, novo) = replay::aceitar(anterior, hb.boot_id, hb.contador);
            if !veredito.passou() {
                return; // replay/duplicata
            }
            if veredito == replay::Veredito::Reancorado {
                log::info!("[ESPNOW] peer {:02X?} reiniciou (boot_id novo)", info.src_addr);
            }
            if let Some(estado) = novo {
                tabela.insert(
                    *info.src_addr,
                    Peer {
                        hb,
                        // Frescor do PEER e medido pelo relogio local do receptor:
                        // uptime monotonico, valido com ou sem internet.
                        recebido_em: crate::comunicacao::sntp::uptime_s(),
                        replay: estado,
                    },
                );
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
/// `proprio_mac` = MAC STA deste no (coberto pelo HMAC v4; o receptor confere contra
/// o `src_addr` do frame).
pub fn difundir(espnow: &EspNow<'static>, hb: &Heartbeat, proprio_mac: &[u8; 6]) -> Result<()> {
    let key = chave_ativa()
        .ok_or_else(|| anyhow!("[ESPNOW] sem residencia_key (iniciar nao chamado)"))?;
    let payload = hb.serializar_assinado(&key, proprio_mac);
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
/// `agora_uptime` = relogio LOCAL monotonico (`sntp::uptime_s`), nao epoch: o TTL de
/// peer e um delta local e nunca precisou de hora certa.
pub fn montar_coordenacao(agora_uptime: i64, ttl_s: i64) -> (Vec<([u8; 6], TomadaInput)>, f32) {
    let tabela = peers().lock().unwrap();
    let mut vivos = Vec::new();
    let mut carga_fixa = 0.0f32;
    for (mac, p) in tabela.iter() {
        if agora_uptime - p.recebido_em <= ttl_s {
            vivos.push((*mac, p.hb.para_tomada_input()));
        } else {
            // Degradacao segura: ultimo p_medida conhecido como carga fixa.
            carga_fixa += p.hb.p_medida;
        }
    }
    (vivos, carga_fixa)
}

/// Remove peers nao vistos ha muito tempo (limpeza opcional de RAM).
pub fn esquecer_antigos(agora: i64, max_idade_s: i64) {
    peers()
        .lock()
        .unwrap()
        .retain(|_, p| agora - p.recebido_em <= max_idade_s);
}
