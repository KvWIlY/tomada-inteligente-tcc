//! Transcript do pareamento local (feature 005, D4 / FR-008;
//! `contracts/pareamento-espnow.md` parte B).
//!
//! Entrega a `residencia_key` de uma tomada EXISTENTE (E) para uma tomada NOVA (N)
//! usando so um PIN curto de 8 caracteres como segredo compartilhado, sobre um radio
//! publico. Por que SPAKE2 e nao "x25519 + PIN no HKDF": num PAKE, quem captura o
//! trafego NAO consegue testar PINs offline — cada tentativa exige uma troca ao vivo,
//! limitada pela janela de 60 s e pelas 3 tentativas. Com x25519+HKDF(PIN), o
//! transcript capturado permitiria forca bruta em 8 caracteres e a chave da casa cairia.
//!
//! Confirmacao de chave: o SPAKE2 sozinho NAO detecta PIN errado (cada lado deriva uma
//! chave de sessao diferente e segue feliz). Quem detecta e o AEAD do `PAIR_KEY`: com
//! PIN errado a tag nao valida e N descarta — e essa falha que conta uma das 3
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

/// Rotulo do HKDF — amarra a chave derivada a ESTE protocolo e versao.
const INFO_HKDF: &[u8] = b"tomada-pair-v1";
/// Tamanho da chave da residencia (HMAC dos heartbeats).
pub const TAM_CHAVE: usize = 32;
/// Nonce do ChaCha20-Poly1305.
pub const TAM_NONCE: usize = 12;
/// Tag de confirmacao do `PAIR_ACK`.
pub const TAM_ACK: usize = 16;
/// Tentativas de PIN por janela (FR-009).
pub const MAX_TENTATIVAS: u8 = 3;
/// Comprimento do PIN de pareamento (8 alfanumericos — FR-008). Se o plano B
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

/// N conclui o SPAKE2 com `PAIR_RESP`. Sucesso aqui NAO significa PIN correto — a
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
/// tempo decorrido (uptime — nao depende de hora certa) e os resultados.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Janela {
    aberta_em_uptime: i64,
    tentativas_restantes: u8,
    fechada: bool,
}

/// Duracao da janela em segundos (FR-008).
pub const DURACAO_JANELA_S: i64 = 60;

impl Janela {
    /// Abre (ou REABRE, se o botao for pressionado com a janela ja aberta — FR-008).
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

    pub fn tentativas_restantes(&self) -> u8 {
        self.tentativas_restantes
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    const MAC_N: [u8; 6] = [0x24, 0x6F, 0x28, 0x11, 0x22, 0x33];
    const MAC_E: [u8; 6] = [0x24, 0x6F, 0x28, 0xAA, 0xBB, 0xCC];
    const NONCE: [u8; TAM_NONCE] = [7; TAM_NONCE];
    const CHAVE_CASA: [u8; TAM_CHAVE] = [0x5A; TAM_CHAVE];

    /// Roda o protocolo inteiro em processo unico. Devolve o transcript (tudo que
    /// trafegou pelo radio) junto do resultado — e o transcript que os testes de
    /// seguranca inspecionam.
    fn rodar(pin_nova: &str, pin_existente: &str) -> (Result<[u8; TAM_CHAVE], ErroPareamento>, Vec<Vec<u8>>) {
        let mut transcript = Vec::new();

        let (est_n, req) = iniciar_nova(pin_nova, &MAC_N, &MAC_E);
        transcript.push(req.clone());

        let (sessao_e, resp) = match responder_existente(pin_existente, &MAC_N, &MAC_E, &req) {
            Ok(x) => x,
            Err(e) => return (Err(e), transcript),
        };
        transcript.push(resp.clone());

        let chave_sessao_n = match concluir_nova(est_n, &resp) {
            Ok(k) => k,
            Err(e) => return (Err(e), transcript),
        };

        let cifrado = embrulhar_chave(&sessao_e, &NONCE, &CHAVE_CASA);
        transcript.push(cifrado.clone());

        let recebida = match desembrulhar_chave(&chave_sessao_n, &NONCE, &cifrado) {
            Ok(k) => k,
            Err(e) => return (Err(e), transcript),
        };

        let tag = ack(&recebida, &MAC_N);
        transcript.push(tag.to_vec());
        if let Err(e) = verificar_ack(&CHAVE_CASA, &MAC_N, &tag) {
            return (Err(e), transcript);
        }
        (Ok(recebida), transcript)
    }

    #[test]
    fn pin_correto_entrega_a_chave_da_casa_intacta() {
        let (r, _) = rodar("K7PQ2XMR", "K7PQ2XMR");
        assert_eq!(r.expect("pareamento deveria concluir"), CHAVE_CASA);
    }

    #[test]
    fn pin_errado_nao_entrega_a_chave() {
        let (r, _) = rodar("AAAAAAAA", "K7PQ2XMR");
        assert_eq!(
            r,
            Err(ErroPareamento::PinIncorreto),
            "a tag AEAD e quem detecta o PIN errado"
        );
    }

    #[test]
    fn transcript_nao_contem_o_pin_nem_a_chave_em_claro() {
        // Verificacao do SC-007 (metodo definido na spec): nenhum byte do PIN ou da
        // chave da casa aparece em claro em NENHUM quadro que trafega pelo radio.
        let pin = "K7PQ2XMR";
        let (r, transcript) = rodar(pin, pin);
        assert!(r.is_ok());

        for (i, quadro) in transcript.iter().enumerate() {
            assert!(
                !contem(quadro, pin.as_bytes()),
                "quadro {i} vazou o PIN em claro"
            );
            assert!(
                !contem(quadro, &CHAVE_CASA),
                "quadro {i} vazou a chave da casa em claro"
            );
        }
    }

    fn contem(agulha_em: &[u8], agulha: &[u8]) -> bool {
        agulha_em.windows(agulha.len()).any(|w| w == agulha)
    }

    #[test]
    fn transcript_de_outro_par_de_macs_nao_serve() {
        // As identidades entram no SPAKE2: replay do handshake com outro par falha.
        let pin = "K7PQ2XMR";
        let (_, req) = iniciar_nova(pin, &MAC_N, &MAC_E);
        let outro_mac: [u8; 6] = [1, 2, 3, 4, 5, 6];
        let (sessao_outro, resp) =
            responder_existente(pin, &outro_mac, &MAC_E, &req).expect("SPAKE2 aceita a msg");
        // Chaves de sessao divergem => o AEAD nao valida.
        let cifrado = embrulhar_chave(&sessao_outro, &NONCE, &CHAVE_CASA);
        let (est_n2, _) = iniciar_nova(pin, &MAC_N, &MAC_E);
        let k_n = concluir_nova(est_n2, &resp).unwrap_or_default();
        assert_eq!(
            desembrulhar_chave(&k_n, &NONCE, &cifrado),
            Err(ErroPareamento::PinIncorreto)
        );
    }

    #[test]
    fn payload_adulterado_e_rejeitado_pela_tag() {
        let pin = "K7PQ2XMR";
        let (est_n, req) = iniciar_nova(pin, &MAC_N, &MAC_E);
        let (sessao_e, resp) = responder_existente(pin, &MAC_N, &MAC_E, &req).unwrap();
        let k_n = concluir_nova(est_n, &resp).unwrap();
        let mut cifrado = embrulhar_chave(&sessao_e, &NONCE, &CHAVE_CASA);
        cifrado[0] ^= 0xFF; // bit flip no radio

        assert_eq!(
            desembrulhar_chave(&k_n, &NONCE, &cifrado),
            Err(ErroPareamento::PinIncorreto)
        );
    }

    #[test]
    fn ack_prova_que_a_nova_ficou_com_a_chave_certa() {
        let boa = ack(&CHAVE_CASA, &MAC_N);
        assert_eq!(verificar_ack(&CHAVE_CASA, &MAC_N, &boa), Ok(()));

        let outra_chave = [0x11; TAM_CHAVE];
        let ruim = ack(&outra_chave, &MAC_N);
        assert_eq!(
            verificar_ack(&CHAVE_CASA, &MAC_N, &ruim),
            Err(ErroPareamento::AckInvalido)
        );
        assert_eq!(
            verificar_ack(&CHAVE_CASA, &MAC_N, &[0u8; 4]),
            Err(ErroPareamento::AckInvalido)
        );
    }

    // ---- janela (FR-009) ----

    #[test]
    fn janela_expira_em_60s() {
        let j = Janela::abrir(1000);
        assert!(j.aberta(1000));
        assert!(j.aberta(1060));
        assert!(!j.aberta(1061), "61 s depois ja fechou");
    }

    #[test]
    fn tres_pins_errados_fecham_a_janela() {
        let mut j = Janela::abrir(0);
        j.falhou();
        assert!(j.aberta(1) && j.tentativas_restantes() == 2);
        j.falhou();
        assert!(j.aberta(2) && j.tentativas_restantes() == 1);
        j.falhou();
        assert!(!j.aberta(3), "3a falha fecha (FR-009)");
    }

    #[test]
    fn botao_com_janela_aberta_reinicia_os_60s() {
        let j = Janela::abrir(1000);
        assert!(!j.aberta(1100));
        let j2 = Janela::abrir(1090); // botao pressionado de novo
        assert!(j2.aberta(1100));
        assert_eq!(j2.tentativas_restantes(), MAX_TENTATIVAS);
    }

    #[test]
    fn concluir_fecha_a_janela() {
        let mut j = Janela::abrir(0);
        j.concluir();
        assert!(!j.aberta(1));
    }

    #[test]
    fn fora_da_janela_nao_ha_pareamento_mesmo_com_pin_certo() {
        // A checagem e do chamador (firmware): a janela fechada nem chega a iniciar o
        // handshake. Aqui garantimos o predicado que ele consulta.
        let j = Janela::abrir(0);
        assert!(!j.aberta(DURACAO_JANELA_S + 1));
    }
}
