//! `gerar-base`: perfis declarados no caso -> CSV deterministico (FR-009, research D7).
//! As replicas de um grupo se diferenciam por `defasagem_min x indice`. Com `variacao_min`,
//! `variacao_duracao` ou `janelas_fds`, cada dia tem a sua rotina, sorteada de forma
//! reproduzivel a partir de `base.semente` e do nome da coluna.

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufWriter, Write};

use anyhow::{Context, Result};
use chrono::{Datelike, Duration, Timelike, Weekday};

use crate::caso::{invalido, parse_janela, Caso, GrupoToml};
use crate::formato::num;

enum Perfil {
    Constante(f64),
    /// Intervalos de uso `[ini, fim)` em `t_s`, ordenados e sem sobreposicao; `cursor` anda
    /// junto com as linhas, que sao crescentes.
    Janelas { uso_w: f64, standby_w: f64, usos: Vec<(i64, i64)>, cursor: usize },
}

impl Perfil {
    fn valor(&mut self, t_s: i64) -> f64 {
        match self {
            Perfil::Constante(w) => *w,
            Perfil::Janelas { uso_w, standby_w, usos, cursor } => {
                while *cursor < usos.len() && usos[*cursor].1 <= t_s {
                    *cursor += 1;
                }
                let dentro = *cursor < usos.len() && usos[*cursor].0 <= t_s;
                if dentro { *uso_w } else { *standby_w }
            }
        }
    }
}

/// splitmix64: sorteio sem dependencia, igual em qualquer maquina.
pub fn sorteio(mut x: u64) -> f64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    (x >> 11) as f64 / (1u64 << 53) as f64
}

/// Os usos de uma coluna ao longo da rodada: as janelas de cada dia (as de fim de semana no
/// sabado e no domingo), cada uma deslocada e esticada pelo sorteio daquele dia.
fn usos(caso: &Caso, g: &GrupoToml, coluna: &str, deslocamento_s: i64) -> Vec<(i64, i64)> {
    let semana: Vec<(u32, u32)> = g.janelas.iter().filter_map(|j| parse_janela(j)).collect();
    let fds: Option<Vec<(u32, u32)>> = g.janelas_fds.as_ref().map(|v| v.iter().filter_map(|j| parse_janela(j)).collect());
    let mut h = caso.base_semente ^ 0xCBF2_9CE4_8422_2325;
    for b in coluna.bytes() {
        h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01B3);
    }
    let meia_noite0 = -(caso.inicio.num_seconds_from_midnight() as i64);
    let dias = caso.duracao_s / 86_400 + 2;
    let mut v = Vec::new();
    // O dia -1 entra por causa das janelas que atravessam a meia-noite.
    for d in -1..=dias {
        let data = caso.inicio.date() + Duration::days(d);
        let fim_de_semana = matches!(data.weekday(), Weekday::Sat | Weekday::Sun);
        let janelas = match (&fds, fim_de_semana) {
            (Some(f), true) => f,
            _ => &semana,
        };
        for (k, &(a, b)) in janelas.iter().enumerate() {
            let semente = h ^ ((d + 1) as u64).wrapping_mul(0x1_0000_0001) ^ (k as u64) << 48;
            let desvio = (g.variacao_min * 60) as f64 * (2.0 * sorteio(semente) - 1.0);
            let fator = 1.0 + g.variacao_duracao * (2.0 * sorteio(semente ^ 0xA5A5_A5A5) - 1.0);
            let duracao = ((b as i64 - a as i64).rem_euclid(1440) * 60) as f64 * fator;
            let ini = meia_noite0 + d * 86_400 + a as i64 * 60 + deslocamento_s + desvio.round() as i64;
            v.push((ini, ini + duracao.round() as i64));
        }
    }
    v.sort_unstable();
    let mut unidos: Vec<(i64, i64)> = Vec::with_capacity(v.len());
    for (a, b) in v {
        match unidos.last_mut() {
            Some(u) if a <= u.1 => u.1 = u.1.max(b),
            _ => unidos.push((a, b)),
        }
    }
    unidos
}

/// Devolve `(linhas, colunas)` escritas.
pub fn gerar(caso: &Caso, forcar: bool) -> Result<(usize, usize)> {
    let destino = &caso.base_arquivo;
    if destino.exists() && !forcar {
        return Err(invalido(format!("{} já existe; use --forcar para sobrescrever", destino.display())));
    }

    let mut vistas = HashSet::new();
    let mut colunas: Vec<(String, Perfil)> = Vec::new();
    for t in &caso.tomadas {
        if !vistas.insert(t.coluna.clone()) {
            continue;
        }
        let g = &caso.grupos[t.grupo_idx];
        let falta = |campo: &str| invalido(format!("grupo \"{}\": perfil {} sem `{campo}`", g.nome, g.perfil.as_deref().unwrap_or("")));
        let perfil = match g.perfil.as_deref() {
            None => return Err(invalido(format!("grupo \"{}\": sem `perfil`, não dá para gerar a base", g.nome))),
            Some("constante") => Perfil::Constante(g.w.ok_or_else(|| falta("w"))?),
            Some(_) => {
                // Sem janelas de semana vale, se houver as de fim de semana (uso so no sabado e domingo).
                if g.janelas.is_empty() && g.janelas_fds.as_ref().map_or(true, |f| f.is_empty()) {
                    return Err(falta("janelas"));
                }
                Perfil::Janelas {
                    uso_w: g.uso_w.ok_or_else(|| falta("uso_w"))?,
                    standby_w: g.standby_w.ok_or_else(|| falta("standby_w"))?,
                    usos: usos(caso, g, &t.coluna, t.replica as i64 * g.defasagem_min * 60),
                    cursor: 0,
                }
            }
        };
        colunas.push((t.coluna.clone(), perfil));
    }

    let passo = caso.base_passo_s;
    let linhas = ((caso.duracao_s + passo - 1) / passo) as usize;
    if let Some(pasta) = destino.parent() {
        if !pasta.as_os_str().is_empty() {
            std::fs::create_dir_all(pasta).with_context(|| format!("criando {}", pasta.display()))?;
        }
    }
    let arquivo = File::create(destino).with_context(|| format!("criando {}", destino.display()))?;
    let mut w = BufWriter::new(arquivo);
    write!(w, "t_s")?;
    for (nome, _) in &colunas {
        write!(w, ",{nome}")?;
    }
    writeln!(w)?;
    for r in 0..linhas {
        let t_s = r as i64 * passo;
        write!(w, "{t_s}")?;
        for (_, perfil) in &mut colunas {
            write!(w, ",{}", num(perfil.valor(t_s), 3))?;
        }
        writeln!(w)?;
    }
    w.flush()?;
    Ok((linhas, colunas.len()))
}
