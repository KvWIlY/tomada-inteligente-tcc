//! Retencao/purga local dos baldes de medicao (FR-014, SC-006/SC-007; research §6).
//!
//! Sao DOIS gatilhos, com papeis diferentes:
//!
//! 1. **Espaco** (a cada chamada, barato): quando o livre cai abaixo de
//!    `GATILHO_LIVRE_PCT`, remove baldes — mais antigo primeiro, preferindo os ja
//!    confirmados — ate o livre voltar a `ALVO_LIVRE_PCT`. A decisao de QUAIS baldes
//!    e pura e host-testada em `tomada_solver::retencao`.
//! 2. **Idade** (a cada 5 dias, marcador `/lfs/last_purge.txt`): remove balde
//!    totalmente confirmado com idade >= 5 dias, e qualquer balde com idade > 7 dias
//!    (cap rigido, mesmo sem confirmacao).
//!
//! Sem o gatilho de espaco, a aritmetica nao fecha: a 1 medicao/min (~238 KB/dia) uma
//! particao de 640 KB enche em ~2,5 dias, e a purga por idade so agiria no 5o — havia
//! uma janela de ~2,5 dias em que a gravacao local falhava e nada limpava nada.
//!
//! Granularidade por balde (1 arquivo/dia) torna a purga O(1) por dia (um `remove`).

use anyhow::{Context, Result};
use std::io::Write;

use tomada_solver::retencao::planejar_por_espaco;

use crate::persistencia::{littlefs, medicao};

const INTERVALO_PURGA_S: i64 = 5 * medicao::DIA_S;
const CAP_RIGIDO_DIAS: i64 = 7;
const IDADE_PURGA_DIAS: i64 = 5;

/// Abaixo desta fracao de espaco livre, a purga por espaco dispara.
/// 15% de 640 KB ~ 96 KB ~ 10 h de medicao — folga suficiente para varios ticks.
const GATILHO_LIVRE_PCT: usize = 15;
/// Fracao de espaco livre que a purga tenta restabelecer. Acima do gatilho para nao
/// purgar a cada tick (histerese); um balde de dia cheio (~238 KB) ja cobre a diferenca.
const ALVO_LIVRE_PCT: usize = 30;

/// O que uma passada de purga fez — material de observabilidade para o `main`.
#[derive(Debug, Default)]
pub struct Resumo {
    /// Baldes removidos pelo gatilho de idade.
    pub por_idade: usize,
    /// Baldes removidos pelo gatilho de espaco.
    pub por_espaco: usize,
    /// Destes, quantos tinham registro NAO confirmado — perda real de dado.
    pub baldes_com_perda: usize,
    /// Bytes liberados pelo gatilho de espaco.
    pub bytes_liberados: u64,
    /// A purga por espaco disparou e mesmo assim nao atingiu o alvo: a gravacao
    /// local ainda pode falhar.
    pub sem_espaco: bool,
}

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

/// Passada de retencao. Chamavel a cada tick de nuvem: o gatilho de espaco e barato
/// (uma consulta ao LittleFS) e o de idade continua limitado a 1x a cada 5 dias.
/// `agora` = epoch atual (SNTP; uptime enquanto nao ha hora certa).
///
/// Deve rodar ANTES do flush do buffer de RAM — e justamente para o flush ter onde
/// gravar que a purga por espaco existe.
pub fn talvez_purgar(agora: i64) -> Result<Resumo> {
    let mut r = Resumo::default();

    // 1. Espaco: toda chamada. Nao depende de relogio, entao funciona igual sem SNTP.
    if let Err(e) = purgar_por_espaco(agora, &mut r) {
        log::warn!("[RETENCAO] purga por espaco falhou: {e:?}");
    }

    // 2. Idade: cara (le todos os baldes), so a cada 5 dias.
    if agora - ultima_purga() >= INTERVALO_PURGA_S {
        r.por_idade = purgar(agora)?;
        registrar_purga(agora)?;
    }

    Ok(r)
}

/// Gatilho de espaco: libera disco antes que a gravacao local comece a falhar.
///
/// O dado de um balde confirmado ja esta no banco — apaga-lo nao perde nada. Se ainda
/// faltar espaco depois de esgotar os confirmados, apaga os nao confirmados do mesmo
/// jeito (o combinado: perder o dado mais antigo e melhor que parar de medir), mas
/// isso vira evento de observabilidade em `Resumo::baldes_com_perda`.
fn purgar_por_espaco(agora: i64, r: &mut Resumo) -> Result<()> {
    let (total, usado) = littlefs::espaco()?;
    let livre = total.saturating_sub(usado) as u64;
    let gatilho = (total * GATILHO_LIVRE_PCT / 100) as u64;
    if livre >= gatilho {
        return Ok(());
    }

    let alvo = (total * ALVO_LIVRE_PCT / 100) as u64;
    let baldes = medicao::inventario()?;
    let plano = planejar_por_espaco(&baldes, livre, alvo, agora / medicao::DIA_S);

    log::warn!(
        "[RETENCAO] espaco baixo: livre={livre}B/{total}B (gatilho={gatilho}B, alvo={alvo}B) \
         -- removendo {} balde(s)",
        plano.remover.len()
    );

    // Falha em um balde nao aborta os demais: liberar parte do espaco ja ajuda.
    for dia in &plano.remover {
        match medicao::remover_balde(*dia) {
            Ok(()) => {
                r.por_espaco += 1;
                if let Some(b) = baldes.iter().find(|b| b.dia == *dia) {
                    r.bytes_liberados += b.bytes;
                    if !b.todos_confirmados {
                        r.baldes_com_perda += 1;
                    }
                }
                log::info!("[RETENCAO] balde dia={dia} removido por espaco");
            }
            Err(e) => log::error!("[RETENCAO] falha removendo balde dia={dia}: {e:?}"),
        }
    }

    r.sem_espaco = !plano.suficiente;
    if r.sem_espaco {
        log::error!(
            "[RETENCAO] purga por espaco NAO atingiu o alvo ({alvo}B) -- a gravacao local pode falhar"
        );
    }
    Ok(())
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
            medicao::remover_balde(dia)?;
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
