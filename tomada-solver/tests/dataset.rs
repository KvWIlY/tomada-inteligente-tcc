//! Validacao empirica do solver contra o dataset sintetico rotulado do orientador.
//!
//! Reproduzivel: CSV gerado por `gerador_dataset_iot.py` (seed 42), 2016 instantes x 4
//! tomadas. Roda no host: `cargo test -p tomada-solver --test dataset`.
//!
//! Estrategia: em vez de fixar a saida esperada por instante (fragil a pesos), asserimos
//! os INVARIANTES estruturais do modelo (R2/R3/R4/R5) sobre TODOS os instantes e sob uma
//! VARREDURA de tetos `p_max` — e adicionamos uma checagem dirigida do gabarito (`cenario
//! _injetado`) que demonstra a reacao do R4 (fantasma: STD -> OFF). A corretude numerica
//! do B&B ja e coberta por `tests/modelo.rs` (cross-check vs. enumeracao 3^N).

mod support;

use support::{carregar, Instante};
use tomada_solver::{resolver, Estado, TipoPenalidade};

/// Tetos varridos (W). Apertados o suficiente p/ a mochila morder; o maior (1500) cobre
/// o pico do Ar_Cond sozinho. Soma de todos em ON = 1885 W.
const TETOS: [f64; 5] = [300.0, 400.0, 600.0, 900.0, 1500.0];

const EPS: f64 = 1e-6;

/// Consumo dos estados escolhidos (W), na escala nominal `C_{i,s}`.
fn consumo(estados: &[Estado], inst: &Instante) -> f64 {
    estados
        .iter()
        .zip(&inst.tomadas)
        .map(|(e, t)| match e {
            Estado::Off => 0.0,
            Estado::Std => t.input.c_std,
            Estado::On => t.input.c_on,
        })
        .sum()
}

/// Invariantes do modelo sobre o dataset inteiro, para cada teto da varredura.
#[test]
fn invariantes_modelo_no_dataset_inteiro() {
    let instantes = carregar();
    assert_eq!(instantes.len(), 2016, "esperado 2016 instantes (7 dias x 288/dia)");
    for inst in &instantes {
        assert_eq!(inst.tomadas.len(), 4, "esperado 4 tomadas por instante");
    }

    for &p_max in &TETOS {
        for inst in &instantes {
            let entrada = inst.solver_input(p_max);
            let out = resolver(&entrada);

            // R1: exatamente um estado por tomada (garantido pelo tipo de retorno).
            assert_eq!(out.estados.len(), inst.tomadas.len());
            assert!(out.factivel, "modelo deve ser sempre factivel (folgas)");

            // R2: consumo <= teto (carga_fixa = 0).
            let c = consumo(&out.estados, inst);
            assert!(
                c <= p_max + EPS,
                "R2 violado em {} (p_max={p_max}): consumo={c}",
                inst.timestamp
            );

            for (i, t) in inst.tomadas.iter().enumerate() {
                let estado = out.estados[i];

                // R4 (hard): F_i => nunca STD.
                if t.input.f_i {
                    assert_ne!(
                        estado,
                        Estado::Std,
                        "R4 violado: STD sob fantasma em {} ({}, p_max={p_max})",
                        inst.timestamp,
                        t.dispositivo
                    );
                }

                let tem_pen = |tipo: TipoPenalidade| {
                    out.penalidades.iter().any(|p| p.tomada_idx == i && p.tipo == tipo)
                };

                // R3 (soft): OFF sob uso ativo <=> folga y_uso ativada.
                let off_sob_uso = estado == Estado::Off && t.input.u_i;
                assert_eq!(
                    off_sob_uso,
                    tem_pen(TipoPenalidade::Uso),
                    "R3: OFF-sob-uso e y_uso devem coincidir em {} ({})",
                    inst.timestamp,
                    t.dispositivo
                );

                // R5 (soft): ON fora do horario <=> folga y_age ativada.
                let on_fora_horario = estado == Estado::On && !t.input.a_it;
                assert_eq!(
                    on_fora_horario,
                    tem_pen(TipoPenalidade::Agendamento),
                    "R5: ON-fora-horario e y_age devem coincidir em {} ({})",
                    inst.timestamp,
                    t.dispositivo
                );
            }
        }
    }
}

/// Gabarito R4 dirigido: sob teto folgado (a escolha nao e por capacidade), toda tomada
/// com fantasma detectado (F_i=1) e sem uso ativo (U_i=0) deve ir a OFF — provando a
/// reacao STD -> OFF que o parecer do orientador pede.
#[test]
fn gabarito_fantasma_vai_para_off() {
    const TETO_FOLGADO: f64 = 5000.0; // > soma de todos em ON: R2 nunca morde.
    let instantes = carregar();

    let mut casos = 0usize;
    for inst in &instantes {
        let out = resolver(&inst.solver_input(TETO_FOLGADO));
        for (i, t) in inst.tomadas.iter().enumerate() {
            if t.input.f_i && !t.input.u_i {
                casos += 1;
                assert_eq!(
                    out.estados[i],
                    Estado::Off,
                    "fantasma sem uso deve ir a OFF em {} ({}, cenario={})",
                    inst.timestamp,
                    t.dispositivo,
                    t.cenario
                );
            }
        }
    }

    assert!(casos > 0, "dataset deveria conter casos de fantasma (F_i=1, U_i=0)");
    eprintln!("gabarito_fantasma_vai_para_off: {casos} casos de fantasma validados");
}

/// Sanidade do gabarito do CSV: existem instantes anomalos rotulados (R4 e R5).
#[test]
fn dataset_contem_cenarios_rotulados() {
    let instantes = carregar();
    let mut fantasma = 0usize;
    let mut fora_agenda = 0usize;
    for inst in &instantes {
        for t in &inst.tomadas {
            match t.cenario.as_str() {
                "Consumo_Fantasma_Ativo" => fantasma += 1,
                "Uso_Fora_Agendamento" => fora_agenda += 1,
                _ => {}
            }
        }
    }
    assert!(fantasma > 0, "esperava linhas Consumo_Fantasma_Ativo (R4)");
    assert!(fora_agenda > 0, "esperava linhas Uso_Fora_Agendamento (R5)");
    eprintln!("rotulos: fantasma={fantasma} fora_agenda={fora_agenda}");
}
