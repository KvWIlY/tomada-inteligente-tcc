//! Hashing de credenciais. Senha de usuário: Argon2id (baixa entropia → KDF
//! lento). `device_secret`/`claim_code`: sha256 (alta entropia CSPRNG → hash
//! rápido resistente a pré-imagem), comparado em tempo constante.
use argon2::password_hash::{rand_core::OsRng as ArgonOsRng, PasswordHash, SaltString};
use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use rand::RngCore;
use sha2::{Digest, Sha256};

pub fn hash_senha(senha: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut ArgonOsRng);
    Argon2::default()
        .hash_password(senha.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| e.to_string())
}

pub fn verificar_senha(senha: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(ph) => Argon2::default()
            .verify_password(senha.as_bytes(), &ph)
            .is_ok(),
        Err(_) => false,
    }
}

/// sha256 hex de um segredo (device_secret / claim_code).
pub fn sha256_hex(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

/// Comparação em tempo constante de duas strings (evita timing attack em hashes).
pub fn comparar_ct(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut r = 0u8;
    for i in 0..a.len() {
        r |= a[i] ^ b[i];
    }
    r == 0
}

/// Segredo aleatório (32 bytes CSPRNG) em hex — usado no provisionamento (US2).
pub fn gerar_segredo() -> String {
    let mut b = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut b);
    hex::encode(b)
}
