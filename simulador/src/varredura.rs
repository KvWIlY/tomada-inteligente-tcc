//! `varredura <caso>` (FR-016, research D11): custo de um `decidir` contra o numero de
//! tomadas. As tomadas do caso sao replicadas em ciclo ate N e o `p_max` escala por N/M,
//! para a curva nao misturar "mais tomadas" com "teto mais justo".

use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result};

use crate::caso::{self, invalido, mac_sintetico, TomadaSim};
use crate::formato::{milhar, num, virgula};
use crate::laco::{estatisticas, Cenario, Laco};
use crate::progresso::{Monitor, Projecao};
use crate::base;

pub fn parse_tamanhos(texto: &str) -> Result<Vec<usize>> {
    let mut v = Vec::new();
    for parte in texto.split(',') {
        let n: usize = parte
            .trim()
            .parse()
            .map_err(|_| invalido(format!("--tamanhos: \"{}\" não é inteiro positivo", parte.trim())))?;
        if n == 0 || n > caso::MAX_TOMADAS {
            return Err(invalido(format!("--tamanhos: {n} fora de 1..{}", caso::MAX_TOMADAS)));
        }
        v.push(n);
    }
    Ok(v)
}

pub fn varredura(caminho: &Path, tamanhos: &[usize], passos: u64, saida: &Path, progresso_s: u64) -> Result<i32> {
    if passos == 0 {
        return Err(invalido("--passos tem que ser maior que zero"));
    }
    let caso = caso::carregar(caminho)?;
    let base = base::carregar(&caso)?;
    let m = caso.tomadas.len();
    let passos = passos.min(caso.passos_total());
    println!(
        "varredura de {}: {} tomadas no caso, {} passos de {} s por tamanho",
        caso.nome,
        m,
        milhar(passos),
        caso.passo_s
    );

    let mut csv = String::from("n_tomadas,variaveis_binarias,decidir_chamadas,mediana_us,p90_us,max_us,p_max_w\n");
    let mut tabela = String::new();
    let _ = writeln!(tabela, "{:>9} {:>10} {:>12} {:>10} {:>10} {:>10} {:>12}", "tomadas", "binarias", "chamadas", "mediana_us", "p90_us", "max_us", "p_max_w");
    for &n in tamanhos {
        let p_max_w = caso.p_max_w * n as f64 / m as f64;
        let tomadas: Vec<TomadaSim> = (0..n)
            .map(|j| {
                let mut t = caso.tomadas[j % m].clone();
                t.nome = format!("{}-r{j}", t.nome);
                t.mac = mac_sintetico(j);
                t.config.p_max = p_max_w as f32;
                t
            })
            .collect();
        let colunas = (0..n).map(|j| base.indices[j % m]).collect();
        let cen = Cenario {
            inicio: caso.inicio,
            passo_s: caso.passo_s,
            passos_total: passos,
            p_max_w: p_max_w as f32,
            carga_fixa_w: caso.carga_fixa_w as f32,
            reservas: caso.reservas.clone(),
            usuario: caso.usuario,
            perdas: caso.perdas.clone(),
            perda_semente: caso.perda_semente,
            tomadas,
            colunas,
        };
        eprintln!("tamanho {n}: {} solves", milhar(passos * n as u64));
        let monitor = Monitor::iniciar(progresso_s, passos, n, Projecao::Nenhuma);
        let mut laco = Laco::novo(&cen, &base, None, false, Some(monitor.estado()))?;
        while laco.passo < passos {
            laco.passo()?;
        }
        drop(monitor);
        let (mediana, p90, max) = estatisticas(&laco.duracoes);
        let chamadas = laco.duracoes.len();
        let _ = writeln!(csv, "{n},{},{},{mediana},{p90},{max},{}", 3 * n, chamadas, num(p_max_w, 3));
        let _ = writeln!(
            tabela,
            "{:>9} {:>10} {:>12} {:>10} {:>10} {:>10} {:>12}",
            n,
            3 * n,
            milhar(chamadas),
            milhar(mediana as u64),
            milhar(p90 as u64),
            milhar(max as u64),
            virgula(p_max_w, 1)
        );
    }

    let dir = saida
        .join(&caso.nome)
        .join(format!("{}-varredura", chrono::Local::now().format("%Y%m%d-%H%M%S")));
    std::fs::create_dir_all(&dir).with_context(|| format!("criando {}", dir.display()))?;
    std::fs::write(dir.join("curva-custo.csv"), &csv).context("gravando curva-custo.csv")?;
    print!("{tabela}");
    println!("tempo de PC (x86_64, opt-level 3); nao traduz o Xtensa");
    println!("gravado em {}", dir.join("curva-custo.csv").display());
    Ok(0)
}
