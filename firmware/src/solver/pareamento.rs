//! Transcript do pareamento local (feature 005, D4 / FR-008;
//! `contracts/pareamento-espnow.md` parte B).
//!
//! Entrega a `residencia_key` de uma tomada EXISTENTE (E) para uma tomada NOVA (N)
//! usando so um PIN curto de 8 caracteres como segredo compartilhado, sobre um radio
//! publico. Por que SPAKE2 e nao "x25519 + PIN no HKDF": num PAKE, quem captura o
//! trafego NAO consegue testar PINs offline - cada tentativa exige uma troca ao vivo,
//! limitada pela janela de 60 s e pelas 3 tentativas. Com x25519+HKDF(PIN), o
//! transcript capturado permitiria forca bruta em 8 caracteres e a chave da casa cairia.
//!
//! Confirmacao de chave: o SPAKE2 sozinho NAO detecta PIN errado (cada lado deriva uma
//! chave de sessao diferente e segue feliz). Quem detecta e o AEAD do `PAIR_KEY`: com
//! PIN errado a tag nao valida e N descarta - e essa falha que conta uma das 3
//! tentativas da janela.
//!
//! Modulo PURO (Principio IX): os dois papeis rodam em processo unico nos testes. Toda
//! aleatoriedade que precisa ser reproduzivel (nonce) entra por parametro; o firmware
//! passa `esp_random`.

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use spake2::{Ed25519Group, Identity, Password, Spake2};

type HmacSha256 = Hmac<Sha256>;

/// Rotulo do HKDF - amarra a chave derivada a ESTE protocolo e versao.
const INFO_HKDF: &[u8] = b"tomada-pair-v1";
/// Tamanho da chave da residencia (HMAC dos heartbeats).
pub const TAM_CHAVE: usize = 32;
/// Nonce do ChaCha20-Poly1305.
pub const TAM_NONCE: usize = 12;
/// Tag de confirmacao do `PAIR_ACK`.
pub const TAM_ACK: usize = 16;
/// Tentativas de PIN por janela (FR-009).
pub const MAX_TENTATIVAS: u8 = 3;
/// Comprimento do PIN de pareamento (8 alfanumericos - FR-008). Se o plano B
/// (x25519) for adotado no spike T002, sobe para 12 (research D4).
pub const TAM_PIN_ESPERADO: usize = 8;

/// Falhas do pareamento. Viram `pareamento_falha{motivo}` no log (Principio VII).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErroPareamento {
    /// Mensagem do SPAKE2 malformada/tamanho errado.
    MensagemInvalida,
    /// Tag AEAD nao validou: PIN errado (ou payload adulterado). Consome 1 tentativa.
    PinIncorreto,
    /// Confirmacao final de N nao bateu.
    AckInvalido,
}

/// Estado do lado NOVA entre `iniciar_nova` e `concluir_nova`.
pub struct EstadoNova {
    spake: Spake2<Ed25519Group>,
}

/// Estado do lado EXISTENTE: ja possui a chave de sessao apos responder.
pub struct SessaoExistente {
    chave_sessao: Vec<u8>,
}

/// Identidades do handshake: os MACs amarram a sessao a este par de dispositivos
/// (um transcript gravado nao serve para outro par).
fn ids(mac_nova: &[u8; 6], mac_existente: &[u8; 6]) -> (Identity, Identity) {
    (Identity::new(mac_nova), Identity::new(mac_existente))
}

/// N inicia: devolve o estado e a mensagem `PAIR_REQ` (parte SPAKE2).
pub fn iniciar_nova(pin: &str, mac_nova: &[u8; 6], mac_existente: &[u8; 6]) -> (EstadoNova, Vec<u8>) {
    let (id_n, id_e) = ids(mac_nova, mac_existente);
    let (spake, msg) = Spake2::<Ed25519Group>::start_a(
        &Password::new(pin.as_bytes()),
        &id_n,
        &id_e,
    );
    (EstadoNova { spake }, msg)
}

/// E responde: consome `PAIR_REQ`, devolve a sessao e a mensagem `PAIR_RESP`.
pub fn responder_existente(
    pin: &str,
    mac_nova: &[u8; 6],
    mac_existente: &[u8; 6],
    msg_req: &[u8],
) -> Result<(SessaoExistente, Vec<u8>), ErroPareamento> {
    let (id_n, id_e) = ids(mac_nova, mac_existente);
    let (spake, msg) = Spake2::<Ed25519Group>::start_b(
        &Password::new(pin.as_bytes()),
        &id_n,
        &id_e,
    );
    let chave_sessao = spake.finish(msg_req).map_err(|_| ErroPareamento::MensagemInvalida)?;
    Ok((SessaoExistente { chave_sessao }, msg))
}

/// N conclui o SPAKE2 com `PAIR_RESP`. Sucesso aqui NAO significa PIN correto - a
/// verificacao real acontece em `desembrulhar_chave`.
pub fn concluir_nova(estado: EstadoNova, msg_resp: &[u8]) -> Result<Vec<u8>, ErroPareamento> {
    estado
        .spake
        .finish(msg_resp)
        .map_err(|_| ErroPareamento::MensagemInvalida)
}

/// Chave de embrulho = HKDF-SHA256(chave de sessao SPAKE2, info do protocolo).
fn chave_embrulho(chave_sessao: &[u8]) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(None, chave_sessao);
    let mut okm = [0u8; 32];
    hk.expand(INFO_HKDF, &mut okm)
        .expect("32 bytes cabem no okm do HKDF-SHA256");
    okm
}

/// E embrulha a chave da casa para o `PAIR_KEY`. `nonce` vem de fora (esp_random no
/// firmware) e trafega em claro junto do texto cifrado.
pub fn embrulhar_chave(
    sessao: &SessaoExistente,
    nonce: &[u8; TAM_NONCE],
    residencia_key: &[u8; TAM_CHAVE],
) -> Vec<u8> {
    let k = chave_embrulho(&sessao.chave_sessao);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&k));
    cipher
        .encrypt(Nonce::from_slice(nonce), residencia_key.as_ref())
        .expect("ChaCha20-Poly1305 nao falha com entradas validas")
}

/// N desembrulha o `PAIR_KEY`. Falha de tag => PIN errado (consome 1 tentativa).
pub fn desembrulhar_chave(
    chave_sessao: &[u8],
    nonce: &[u8; TAM_NONCE],
    cifrado: &[u8],
) -> Result<[u8; TAM_CHAVE], ErroPareamento> {
    let k = chave_embrulho(chave_sessao);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&k));
    let claro = cipher
        .decrypt(Nonce::from_slice(nonce), cifrado)
        .map_err(|_| ErroPareamento::PinIncorreto)?;
    if claro.len() != TAM_CHAVE {
        return Err(ErroPareamento::MensagemInvalida);
    }
    let mut key = [0u8; TAM_CHAVE];
    key.copy_from_slice(&claro);
    Ok(key)
}

/// `PAIR_ACK`: N prova que decifrou a chave certa. E confere com a propria chave.
pub fn ack(residencia_key: &[u8; TAM_CHAVE], mac_nova: &[u8; 6]) -> [u8; TAM_ACK] {
    // Qualificado: `KeyInit` (AEAD) e `Mac` (hmac) exportam `new_from_slice`.
    let mut m = <HmacSha256 as Mac>::new_from_slice(residencia_key).expect("HMAC aceita 32 bytes");
    m.update(b"ack");
    m.update(mac_nova);
    let full = m.finalize().into_bytes();
    let mut t = [0u8; TAM_ACK];
    t.copy_from_slice(&full[..TAM_ACK]);
    t
}

/// Verificacao em tempo ~constante do ACK.
pub fn verificar_ack(
    residencia_key: &[u8; TAM_CHAVE],
    mac_nova: &[u8; 6],
    recebido: &[u8],
) -> Result<(), ErroPareamento> {
    let esperado = ack(residencia_key, mac_nova);
    if recebido.len() != TAM_ACK {
        return Err(ErroPareamento::AckInvalido);
    }
    let mut diff = 0u8;
    for i in 0..TAM_ACK {
        diff |= esperado[i] ^ recebido[i];
    }
    if diff == 0 {
        Ok(())
    } else {
        Err(ErroPareamento::AckInvalido)
    }
}

/// Contabilidade da janela de pareamento (FR-009), pura: o firmware so informa o
/// tempo decorrido (uptime - nao depende de hora certa) e os resultados.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Janela {
    aberta_em_uptime: i64,
    tentativas_restantes: u8,
    fechada: bool,
}

/// Duracao da janela em segundos (FR-008).
pub const DURACAO_JANELA_S: i64 = 60;

impl Janela {
    /// Abre (ou REABRE, se o botao for pressionado com a janela ja aberta - FR-008).
    pub fn abrir(uptime_agora: i64) -> Janela {
        Janela {
            aberta_em_uptime: uptime_agora,
            tentativas_restantes: MAX_TENTATIVAS,
            fechada: false,
        }
    }

    pub fn aberta(&self, uptime_agora: i64) -> bool {
        !self.fechada
            && self.tentativas_restantes > 0
            && (uptime_agora - self.aberta_em_uptime) <= DURACAO_JANELA_S
    }

    /// Registra uma falha de autenticacao; fecha a janela na 3a.
    pub fn falhou(&mut self) {
        self.tentativas_restantes = self.tentativas_restantes.saturating_sub(1);
        if self.tentativas_restantes == 0 {
            self.fechada = true;
        }
    }

    /// Pareamento concluido: fecha.
    pub fn concluir(&mut self) {
        self.fechada = true;
    }
}
