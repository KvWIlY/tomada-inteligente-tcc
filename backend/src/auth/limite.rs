//! T053 — rate-limiting dos endpoints de credencial (`/auth/*`, `/provisionar`,
//! `/renovar`).
//!
//! São as únicas rotas em que um atacante pode adivinhar um segredo por tentativa e
//! erro: senha de usuário, claim-code e `device_secret`. O resto da API já exige um
//! JWT válido, então força bruta ali não tem por onde começar.
//!
//! Janela deslizante em memória, por IP, **global ao processo** — não é `app_data`
//! porque o Actix constrói uma `App` por worker, e uma cota por worker significaria
//! N× o limite real. Escolha deliberada de não usar Redis: o backend é LAN-only e roda
//! num processo só (clarify Q2). Se for exposto à internet com mais de uma instância,
//! isto precisa virar um contador compartilhado.
use crate::erros::ApiErro;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Tentativas permitidas por IP dentro da janela.
const MAX_TENTATIVAS: usize = 10;
/// Tamanho da janela deslizante.
const JANELA: Duration = Duration::from_secs(60);

fn tentativas() -> &'static Mutex<HashMap<String, Vec<Instant>>> {
    static T: OnceLock<Mutex<HashMap<String, Vec<Instant>>>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Registra uma tentativa vinda de `req` e devolve `Err(429)` se estourou a cota.
///
/// Requisições sem `peer_addr` não são limitadas: isso só acontece no harness de
/// teste in-process (`actix_web::test`), onde não há socket. Todo request que chega
/// por TCP tem endereço de origem.
pub fn checar(req: &actix_web::HttpRequest) -> Result<(), ApiErro> {
    let Some(ip) = req.peer_addr().map(|a| a.ip().to_string()) else {
        return Ok(());
    };

    let agora = Instant::now();
    let mut mapa = tentativas().lock().unwrap();

    // Descarta o que saiu da janela no mesmo passo, para o mapa não crescer
    // indefinidamente com IPs que apareceram uma vez.
    mapa.retain(|_, ts| {
        ts.retain(|t| agora.duration_since(*t) < JANELA);
        !ts.is_empty()
    });

    let registros = mapa.entry(ip.clone()).or_default();
    if registros.len() >= MAX_TENTATIVAS {
        log::warn!("rate-limit: {ip} excedeu {MAX_TENTATIVAS} tentativas em {JANELA:?}");
        return Err(ApiErro::MuitasTentativas);
    }
    registros.push(agora);
    Ok(())
}
