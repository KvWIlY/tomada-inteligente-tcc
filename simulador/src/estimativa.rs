//! Estimativa (research D6): roda a rodada de verdade por ate 20 s, mede cada `decidir` e
//! projeta. Tende a SUBESTIMAR: os primeiros passos sao os mais baratos.

use std::time::{Duration, Instant};

use anyhow::Result;

use crate::caso::Caso;
use crate::formato::{duracao_curta, duracao_humana, milhar, virgula};
use crate::laco::{Cenario, Laco};
use crate::progresso::{Monitor, Progresso, Projecao, JANELA_ESTIMATIVA_S};
use crate::{base, caso};

pub fn cabecalho(nome: &str, n_tomadas: usize, passos: u64, passo_s: i64) -> String {
    format!(
        "caso {nome}: {n_tomadas} tomadas, {} passos de {passo_s} s ({}), {} solves",
        milhar(passos),
        duracao_curta(passos as i64 * passo_s),
        milhar(passos * n_tomadas as u64)
    )
}

/// Projecao a partir das chamadas medidas ate aqui, mesmo que o passo em curso nao tenha
/// terminado (SC-002).
pub fn texto_projecao(p: &Progresso) -> String {
    let decorrido = p.inicio_rodada.elapsed().as_secs_f64();
    if p.amostras.len() == 0 {
        return format!(
            "projecao: nenhuma chamada de decidir terminou em {} s; o primeiro solve ainda nao acabou (tomada {}/{})",
            virgula(decorrido, 1),
            p.tomada_atual,
            p.n_tomadas
        );
    }
    let q = p.amostras.percentis(&[0.10, 0.50, 0.90]);
    let (p10, p50, p90) = (q[0], q[1], q[2]);
    let restantes = (p.n_tomadas as u64 * p.passos_total).saturating_sub(p.chamadas) as f64;
    let projetar = |us: u32| decorrido + us as f64 * 1e-6 * restantes;
    format!(
        "medidas {} chamadas de decidir em {} s: {} us cada (p10 {} / p90 {}), {} ms por passo\n\
         projecao: ~{} (faixa {} - {})\n\
         atencao: medido no comeco da base, quando a calibracao ainda esta em zero - tende a SUBESTIMAR",
        milhar(p.amostras.len()),
        virgula(decorrido, 1),
        milhar(p50 as u64),
        milhar(p10 as u64),
        milhar(p90 as u64),
        virgula(p50 as f64 * p.n_tomadas as f64 / 1000.0, 1),
        duracao_humana(projetar(p50)),
        duracao_humana(projetar(p10)),
        duracao_humana(projetar(p90))
    )
}

/// `estimar <caso>`: nada e gravado.
pub fn estimar(caminho: &std::path::Path, progresso_s: u64) -> Result<i32> {
    let caso: Caso = caso::carregar(caminho)?;
    let base = base::carregar(&caso)?;
    for a in caso.avisos.iter().chain(&base.avisos) {
        eprintln!("aviso: {a}");
    }
    let cen = Cenario::de_caso(&caso, &base, caso.passos_total());
    println!("{}", cabecalho(&caso.nome, cen.tomadas.len(), cen.passos_total, cen.passo_s));

    let monitor = Monitor::iniciar(progresso_s, cen.passos_total, cen.tomadas.len(), Projecao::Estimar);
    let estado = monitor.estado();
    let mut laco = Laco::novo(&cen, &base, None, false, Some(monitor.estado()))?;
    let inicio = Instant::now();
    let janela = Duration::from_secs(JANELA_ESTIMATIVA_S);
    while laco.passo < cen.passos_total && inicio.elapsed() < janela {
        if estado.lock().map(|g| g.projecao_impressa).unwrap_or(true) {
            break;
        }
        laco.passo()?;
    }
    {
        let mut g = estado.lock().expect("progresso");
        if !g.projecao_impressa {
            g.projecao_impressa = true;
            if laco.passo >= cen.passos_total {
                println!(
                    "a rodada inteira coube na medicao: {} de parede ({} chamadas de decidir), tempo real e nao projecao",
                    duracao_humana(inicio.elapsed().as_secs_f64()),
                    milhar(g.chamadas)
                );
            } else {
                println!("{}", texto_projecao(&g));
            }
        }
    }
    drop(monitor);
    Ok(0)
}
