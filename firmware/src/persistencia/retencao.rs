//! Retencao/purga local dos baldes de medicao (FR-014, SC-006/SC-007; research §6).
//!
//! Regras:
//!   - Purga roda a cada **5 dias** (marcador `/lfs/last_purge.txt`).
//!   - Remove um balde se TODOS os seus registros estao `confirmado=true` e o dia tem
//!     idade >= 5 dias (purga normal).
//!   - **Cap rigido**: qualquer balde com idade > **7 dias** e removido mesmo sem
//!     confirmacao (protege a flash; aceita perda do dado mais antigo em outage > 7 d).
//!
//! Granularidade por balde (1 arquivo/dia) torna a purga O(1) por dia (um `remove`).

use anyhow::{Context, Result};
use std::io::Write;

use crate::persistencia::{littlefs, medicao};

const INTERVALO_PURGA_S: i64 = 5 * medicao::DIA_S;
const CAP_RIGIDO_DIAS: i64 = 7;
const IDADE_PURGA_DIAS: i64 = 5;

fn caminho_marcador() -> String {
    format!("{}/last_purge.txt", littlefs::base())
}

fn ultima_purga() -> i64 {
    std::fs::read_to_string(caminho_marcador())
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .unwrap_or(0)
}

fn registrar_purga(agora: i64) -> Result<()> {
    let path = caminho_marcador();
    let mut f = std::fs::File::create(&path).with_context(|| format!("criando {path}"))?;
    f.write_all(agora.to_string().as_bytes())?;
    f.flush()?;
    Ok(())
}

/// Roda a purga apenas se passaram >= 5 dias da ultima (chamavel a cada ciclo, barato).
/// `agora` = epoch atual (SNTP). Retorna quantos baldes foram removidos.
pub fn talvez_purgar(agora: i64) -> Result<usize> {
    if agora - ultima_purga() < INTERVALO_PURGA_S {
        return Ok(0);
    }
    let removidos = purgar(agora)?;
    registrar_purga(agora)?;
    Ok(removidos)
}

/// Executa a purga incondicionalmente (util em testes/manutencao).
pub fn purgar(agora: i64) -> Result<usize> {
    let dia_atual = agora / medicao::DIA_S;
    let mut removidos = 0usize;
    for dia in medicao::dias_em_disco()? {
        let idade = dia_atual - dia;
        if idade < IDADE_PURGA_DIAS {
            continue; // muito recente p/ qualquer purga
        }
        let cap = idade > CAP_RIGIDO_DIAS;
        let todos_confirmados = || -> Result<bool> {
            Ok(medicao::ler_balde(dia)?.iter().all(|m| m.confirmado))
        };
        if cap || todos_confirmados()? {
            let path = medicao::caminho_balde(dia);
            std::fs::remove_file(&path).with_context(|| format!("removendo {path}"))?;
            removidos += 1;
            log::info!(
                "[RETENCAO] balde dia={dia} removido (idade={idade}d, cap={cap})"
            );
        }
    }
    if removidos > 0 {
        log::info!("[RETENCAO] purga removeu {removidos} balde(s)");
    }
    Ok(removidos)
}
