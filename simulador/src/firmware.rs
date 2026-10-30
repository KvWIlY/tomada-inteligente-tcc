//! Identificacao dos fontes do firmware que este binario compilou (FR-024b, research D9).

use sha2::{Digest, Sha256};

include!(concat!(env!("OUT_DIR"), "/fontes.rs"));

/// SHA-256 sobre nome e conteudo de cada fonte, em ordem de caminho, com CRLF normalizado
/// para LF: o `core.autocrlf` do Windows nao pode mudar a identidade do firmware.
pub fn fonte_firmware_sha256() -> String {
    let mut h = Sha256::new();
    for (nome, conteudo) in FONTES {
        h.update(nome.as_bytes());
        h.update([0u8]);
        h.update(conteudo.replace("\r\n", "\n").as_bytes());
        h.update([0u8]);
    }
    hex(&h.finalize())
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
