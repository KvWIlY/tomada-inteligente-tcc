//! Experimento do TCC: roda o dataset sintetico inteiro sob uma varredura de tetos
//! `p_max` e despeja CSVs prontos para plotar (reducao de consumo global, decisoes por
//! instante e tempo medio de ciclo).
//!
//! Uso (host):
//!   cargo run -p tomada-solver --example dataset_sweep [-- <dir_saida>]
//!
//! Saidas (default em `./saida_dataset` relativo ao crate):
//!   - decisoes.csv        : 1 linha por (p_max, timestamp, tomada) — serie completa.
//!   - reducao_por_pmax.csv : 1 linha por p_max — energia/reducao/penalidades/tempo.
//!
//! Este runner usa o MESMO `resolver()` que roda no ESP32; o modo-teste por serial
//! (abordagem A) reaproveita esta logica de comparacao trocando a fonte do input.

#[path = "../tests/support/mod.rs"]
mod support;

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant as Cronometro;

use support::{carregar, Tomada};
use tomada_solver::{resolver, Estado, TipoPenalidade};

/// Tetos varridos (W). Veja `tests/dataset.rs`.
const TETOS: [f64; 5] = [300.0, 400.0, 600.0, 900.0, 1500.0];

/// Intervalo de amostragem do dataset (h) — converte potencia (W) em energia (Wh).
const INTERVALO_H: f64 = 5.0 / 60.0;

fn consumo_estado(estado: Estado, t: &Tomada) -> f64 {
    match estado {
        Estado::Off => 0.0,
        Estado::Std => t.input.c_std,
        Estado::On => t.input.c_on,
    }
}

/// Estado "natural" (baseline SEM otimizador): a tomada fica como o sensor a viu —
/// ON sob uso ativo; STD se ha consumo residual (standby/fantasma); OFF se zerada.
/// E a linha de base nominal contra a qual medimos a reducao.
fn estado_natural(t: &Tomada) -> Estado {
    if t.input.u_i {
        Estado::On
    } else if t.p_atual > 0.0 {
        Estado::Std
    } else {
        Estado::Off
    }
}

fn main() -> std::io::Result<()> {
    let dir_saida = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("saida_dataset"));
    fs::create_dir_all(&dir_saida)?;

    let instantes = carregar();
    println!(
        "dataset: {} instantes x {} tomadas | tetos: {:?}",
        instantes.len(),
        instantes.first().map(|i| i.tomadas.len()).unwrap_or(0),
        TETOS
    );

    let mut decisoes = csv_writer(&dir_saida.join("decisoes.csv"))?;
    writeln!(
        decisoes,
        "p_max,timestamp,dispositivo,cenario,estado_natural,estado_otimizado,\
         consumo_natural_w,consumo_otimizado_w,p_atual_w"
    )?;

    let mut resumo = csv_writer(&dir_saida.join("reducao_por_pmax.csv"))?;
    writeln!(
        resumo,
        "p_max,energia_natural_wh,energia_otimizada_wh,energia_medida_wh,reducao_pct,\
         pen_uso,pen_agendamento,fantasma_cortado,tempo_medio_us"
    )?;

    for &p_max in &TETOS {
        let mut e_natural = 0.0;
        let mut e_otim = 0.0;
        let mut e_medida = 0.0;
        let mut pen_uso = 0usize;
        let mut pen_age = 0usize;
        let mut fantasma_cortado = 0usize;
        let mut tempo_total = std::time::Duration::ZERO;

        for inst in &instantes {
            let entrada = inst.solver_input(p_max);
            let crono = Cronometro::now();
            let out = resolver(&entrada);
            tempo_total += crono.elapsed();

            for (i, t) in inst.tomadas.iter().enumerate() {
                let natural = estado_natural(t);
                let otim = out.estados[i];
                let c_nat = consumo_estado(natural, t);
                let c_otim = consumo_estado(otim, t);

                e_natural += c_nat * INTERVALO_H;
                e_otim += c_otim * INTERVALO_H;
                e_medida += t.p_atual * INTERVALO_H;

                // Fantasma cortado: estava em standby no baseline e foi a OFF pelo solver.
                if t.input.f_i && natural == Estado::Std && otim == Estado::Off {
                    fantasma_cortado += 1;
                }

                writeln!(
                    decisoes,
                    "{p_max},{},{},{},{},{},{:.2},{:.2},{:.2}",
                    inst.timestamp,
                    t.dispositivo,
                    t.cenario,
                    natural.as_str(),
                    otim.as_str(),
                    c_nat,
                    c_otim,
                    t.p_atual
                )?;
            }

            for p in &out.penalidades {
                match p.tipo {
                    TipoPenalidade::Uso => pen_uso += 1,
                    TipoPenalidade::Agendamento => pen_age += 1,
                }
            }
        }

        let reducao = if e_natural > 0.0 {
            100.0 * (e_natural - e_otim) / e_natural
        } else {
            0.0
        };
        let tempo_medio_us = tempo_total.as_secs_f64() * 1e6 / instantes.len() as f64;

        writeln!(
            resumo,
            "{p_max},{e_natural:.1},{e_otim:.1},{e_medida:.1},{reducao:.2},\
             {pen_uso},{pen_age},{fantasma_cortado},{tempo_medio_us:.1}"
        )?;

        println!(
            "p_max={p_max:>6.0} W | natural={e_natural:>8.0} Wh -> otim={e_otim:>8.0} Wh \
             | reducao={reducao:>5.1}% | y_uso={pen_uso} y_age={pen_age} \
             fantasma_off={fantasma_cortado} | ciclo~{tempo_medio_us:.0}us"
        );
    }

    decisoes.flush()?;
    resumo.flush()?;
    println!("\nCSVs em: {}", dir_saida.display());
    Ok(())
}

fn csv_writer(caminho: &std::path::Path) -> std::io::Result<std::io::BufWriter<fs::File>> {
    Ok(std::io::BufWriter::new(fs::File::create(caminho)?))
}
