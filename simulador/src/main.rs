//! Simulador da feature 009: roda o MILP do firmware no PC sobre uma base CSV de consumo.
//!
//! Cada tomada chama `Ciclo::decidir` do firmware como ele e, vendo as vizinhas pelo anuncio
//! do passo anterior: N solves por passo (FR-003a). O firmware NAO e tocado (FR-003).

// Fontes do firmware, sem copia (research D1). Ficam na RAIZ do crate porque os arquivos
// se referem uns aos outros por `crate::solver::...`. Dentro de modulo inline, o `#[path]`
// e relativo a `src/<modulo>/`, dai os tres `../`.
//
// O `solver` repete o `firmware/src/solver/mod.rs` em vez de inclui-lo, para o `resolver`
// que o `Ciclo` chama passar por `regras` (comparacao com regra fixa e prioridade). Modulo
// novo no firmware entra aqui tambem.
#[allow(dead_code, unused_imports)]
mod solver {
    #[path = "../../../firmware/src/solver/coordenacao.rs"]
    pub mod coordenacao;
    #[path = "../../../firmware/src/solver/modelo.rs"]
    pub mod modelo;
    #[path = "../../../firmware/src/solver/pareamento.rs"]
    pub mod pareamento;
    #[path = "../../../firmware/src/solver/portal.rs"]
    pub mod portal;
    #[path = "../../../firmware/src/solver/relogio.rs"]
    pub mod relogio;
    #[path = "../../../firmware/src/solver/replay.rs"]
    pub mod replay;
    #[path = "../../../firmware/src/solver/retencao.rs"]
    pub mod retencao;
    #[path = "../../../firmware/src/solver/solver.rs"]
    pub mod solver;
    #[path = "../../../firmware/src/solver/sync.rs"]
    pub mod sync;
    #[path = "../../../firmware/src/solver/tipos.rs"]
    pub mod tipos;
    #[path = "../../../firmware/src/solver/uso_agora.rs"]
    pub mod uso_agora;

    pub use crate::regras::resolver;
    pub use tipos::{Estado, Pesos, SolverInput, SolverOutput, TipoPenalidade, TomadaInput};
}

#[allow(dead_code, unused_imports)]
mod sensores {
    #[path = "../../../firmware/src/sensores/potencia.rs"]
    pub mod potencia;
}

#[allow(dead_code, unused_imports)]
mod persistencia {
    #[path = "../../../firmware/src/persistencia/config.rs"]
    pub mod config;
    /// Enxerto: o `config.rs` chama `littlefs::base()` em `load`/`save`, que o simulador
    /// nunca chama. Existe so para o arquivo compilar.
    pub mod littlefs {
        pub fn base() -> &'static str {
            "."
        }
    }
}

#[allow(dead_code, unused_imports)]
mod decisao {
    #[path = "../../../firmware/src/decisao/ciclo.rs"]
    pub mod ciclo;
}

mod base;
mod caso;
mod estimativa;
mod firmware;
mod formato;
mod gerar_base;
mod histograma;
mod invariantes;
mod laco;
mod progresso;
mod regras;
mod registro;
mod varredura;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use anyhow::Result;
use clap::{Parser, Subcommand};

use caso::{invalido, Invalido};
use formato::{duracao_humana, milhar, virgula};
use invariantes::Regra;
use laco::{Cenario, Desfecho, Laco};
use progresso::{Monitor, Projecao};

/// Duracao a partir da qual `verificar` roda so um recorte do caso (research D12).
const VERIFICAR_INTEIRO_ATE_S: i64 = 48 * 3600;
const VERIFICAR_RECORTE: &str = "6h";

#[derive(Parser)]
#[command(name = "simulador", version, about = "Roda o MILP do firmware no PC sobre uma base CSV de consumo (feature 009)")]
struct Cli {
    #[command(subcommand)]
    comando: Comando,
}

#[derive(Subcommand)]
enum Comando {
    /// Percorre a base do primeiro ao ultimo instante e grava a pasta da rodada.
    Rodar(RodarArgs),
    /// Mede ate 20 s de rodada real, projeta o total e descarta.
    Estimar {
        caso: PathBuf,
        #[arg(long, default_value_t = 5)]
        progresso: u64,
    },
    /// Escreve o CSV da base a partir dos perfis declarados no caso.
    GerarBase {
        caso: PathBuf,
        #[arg(long)]
        forcar: bool,
    },
    /// Curva de custo de um decidir contra o numero de tomadas.
    Varredura {
        caso: PathBuf,
        #[arg(long, default_value = "1,4,8,16,32,64,100")]
        tamanhos: String,
        #[arg(long, default_value_t = 360)]
        passos: u64,
        #[arg(long, default_value = "rodadas")]
        saida: PathBuf,
        #[arg(long, default_value_t = 5)]
        progresso: u64,
    },
    /// Roda os casos com determinismo e expectativas, e depois planta regressoes.
    Verificar {
        alvo: PathBuf,
        #[arg(long, default_value = "rodadas")]
        saida: PathBuf,
        #[arg(long, default_value_t = 5)]
        progresso: u64,
    },
}

#[derive(clap::Args, Clone)]
struct RodarArgs {
    caso: PathBuf,
    /// Acrescenta -<texto> ao nome da pasta.
    #[arg(long)]
    rotulo: Option<String>,
    /// Grava rastro.csv, uma linha por tomada por passo.
    #[arg(long)]
    rastro: bool,
    /// Para no tempo de parede dado; sobrescreve limites.orcamento.
    #[arg(long)]
    orcamento: Option<String>,
    /// Simula so os primeiros N de base.
    #[arg(long)]
    ate: Option<String>,
    /// Roda duas vezes e compara passo a passo.
    #[arg(long)]
    determinismo: bool,
    /// Adultera a decisao para provar que a conferencia pega.
    #[arg(long, value_enum)]
    plantar: Option<Regra>,
    /// Tomada que ficou OFF le 0 W no passo seguinte, como a placa com o rele aberto.
    #[arg(long)]
    realimentar: bool,
    #[arg(long, default_value = "rodadas")]
    saida: PathBuf,
    /// Intervalo do progresso em segundos (maximo 30).
    #[arg(long, default_value_t = 5)]
    progresso: u64,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let resultado = match cli.comando {
        Comando::Rodar(a) => validar_progresso(a.progresso).and_then(|_| rodar(&a)),
        Comando::Estimar { caso, progresso } => {
            validar_progresso(progresso).and_then(|_| estimativa::estimar(&caso, progresso))
        }
        Comando::GerarBase { caso, forcar } => gerar(&caso, forcar),
        Comando::Varredura { caso, tamanhos, passos, saida, progresso } => validar_progresso(progresso)
            .and_then(|_| varredura::parse_tamanhos(&tamanhos))
            .and_then(|t| varredura::varredura(&caso, &t, passos, &saida, progresso)),
        Comando::Verificar { alvo, saida, progresso } => {
            validar_progresso(progresso).and_then(|_| verificar(&alvo, &saida, progresso))
        }
    };
    match resultado {
        Ok(codigo) => ExitCode::from(codigo as u8),
        Err(e) if e.downcast_ref::<Invalido>().is_some() => {
            eprintln!("recusado: {e}");
            ExitCode::from(2)
        }
        Err(e) => {
            eprintln!("erro: {e:#}");
            ExitCode::from(1)
        }
    }
}

fn validar_progresso(s: u64) -> Result<()> {
    if s == 0 || s > 30 {
        return Err(invalido(format!("--progresso = {s}: use de 1 a 30 segundos (FR-014a)")));
    }
    Ok(())
}

fn gerar(caminho: &Path, forcar: bool) -> Result<i32> {
    let caso = caso::carregar(caminho)?;
    for a in &caso.avisos {
        eprintln!("aviso: {a}");
    }
    let (linhas, colunas) = gerar_base::gerar(&caso, forcar)?;
    println!(
        "base gerada: {} ({} linhas, {} colunas, passo {} s)",
        caso.base_arquivo.display(),
        milhar(linhas as u64),
        colunas,
        caso.base_passo_s
    );
    Ok(0)
}

/// Opcoes de uma rodada, de `rodar` ou montadas por `verificar`.
struct Opcoes {
    caso: PathBuf,
    rotulo: Option<String>,
    rastro: bool,
    orcamento: Option<String>,
    ate: Option<String>,
    determinismo: bool,
    plantar: Option<Regra>,
    realimentar: bool,
    saida: PathBuf,
    progresso_s: u64,
    /// `verificar` imprime a propria linha por caso.
    quieto: bool,
}

struct Resultado {
    desfecho: Desfecho,
    motivo: Option<String>,
    pasta: PathBuf,
    recorte: bool,
    /// Violacao de invariante (e nao expectativa, determinismo ou regressao nao plantada).
    violou_invariante: bool,
    expectativas: Option<(usize, usize)>,
}

fn rodar(a: &RodarArgs) -> Result<i32> {
    let r = executar(&Opcoes {
        caso: a.caso.clone(),
        rotulo: a.rotulo.clone(),
        rastro: a.rastro,
        orcamento: a.orcamento.clone(),
        ate: a.ate.clone(),
        determinismo: a.determinismo,
        plantar: a.plantar,
        realimentar: a.realimentar,
        saida: a.saida.clone(),
        progresso_s: a.progresso,
        quieto: false,
    })?;
    Ok(r.desfecho.codigo())
}

fn executar(o: &Opcoes) -> Result<Resultado> {
    if let Some(r) = &o.rotulo {
        if r.is_empty() || !r.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Err(invalido(format!("--rotulo \"{r}\": use letras, dígitos, hífen ou sublinhado")));
        }
    }
    let caso = caso::carregar(&o.caso)?;
    let base = base::carregar(&caso)?;
    let mut avisos = caso.avisos.clone();
    avisos.extend(base.avisos.iter().cloned());
    for av in &avisos {
        eprintln!("aviso: {av}");
    }

    let passos_caso = caso.passos_total();
    let (passos_total, recorte) = match &o.ate {
        Some(texto) => {
            let ate = caso::parse_duracao("--ate", texto)?;
            if ate < caso.passo_s {
                return Err(invalido(format!("--ate {texto}: menor que um passo ({} s)", caso.passo_s)));
            }
            let p = (ate / caso.passo_s) as u64;
            (p.min(passos_caso), p < passos_caso)
        }
        None => (passos_caso, false),
    };
    let orcamento_s = match &o.orcamento {
        Some(texto) => {
            let s = caso::parse_duracao("--orcamento", texto)?;
            if s <= 0 {
                return Err(invalido(format!("--orcamento {texto}: tem que ser maior que zero")));
            }
            Some(s)
        }
        None => caso.orcamento_s,
    };

    regras::definir(caso.decisao);
    let cen = Cenario::de_caso(&caso, &base, passos_total);
    let n = cen.tomadas.len();
    let mut params = registro::Parametros::novo(&caso, &base, &cen, orcamento_s, o.rotulo.clone());
    if o.realimentar {
        params.modo = registro::MODO_REALIMENTA;
    }
    let mut pasta = registro::Pasta::criar(&o.saida, &caso, &params, o.rastro)?;

    if !o.quieto {
        println!("{}", estimativa::cabecalho(&caso.nome, n, passos_total, cen.passo_s));
        println!("{}", registro::frase_modo(params.modo));
        match caso.decisao {
            regras::Modo::Milp => {}
            regras::Modo::Centralizado => println!("decisão: centralizado (o MILP vendo todas as tomadas no passo atual)"),
            d => println!("decisão: {} (regra de comparação, não o MILP)", d.as_str()),
        }
        if let Some(av) = registro::aviso_passo(cen.passo_s) {
            println!("{av}");
        }
        if let Some(p) = o.plantar {
            println!("regressão plantada: {} (a rodada TEM que falhar)", p.nome());
        }
    }

    let intervalo = Duration::from_secs(o.progresso_s);
    let monitor = Monitor::iniciar(o.progresso_s, passos_total, n, if o.quieto { Projecao::Nenhuma } else { Projecao::Rodar });
    let mut laco = Laco::novo(&cen, &base, o.plantar, o.rastro, Some(monitor.estado()))?;
    laco.realimentar = o.realimentar;
    let mut digests = if o.determinismo { Some(registro::Digests::criar(&pasta.dir)?) } else { None };
    let inicio = Instant::now();
    pasta.descarregar(
        &registro::resumo(&cen, &laco, Desfecho::EmAndamento, None, 0.0),
        &registro::tomadas_csv(&cen, &laco),
    )?;
    let mut ultimo_flush = Instant::now();
    let mut desfecho = Desfecho::Completou;
    let mut motivo: Option<String> = None;
    let mut violou_invariante = false;
    while laco.passo < passos_total {
        // O orcamento e conferido entre passos, nunca no meio de um (FR-013).
        if let Some(orc) = orcamento_s {
            if inicio.elapsed().as_secs_f64() >= orc as f64 {
                let ev = laco.evento_casa(
                    "parou_orcamento",
                    "warn",
                    "Orcamento da rodada batido",
                    format!("orcamento_s={orc} ultimo_passo={}", laco.passo),
                );
                pasta.evento(&ev)?;
                desfecho = Desfecho::ParouNoOrcamento;
                break;
            }
        }
        let saida = laco.passo()?;
        if let Some(d) = &mut digests {
            d.gravar(&laco.ultimo_digest)?;
        }
        for ev in &saida.eventos {
            pasta.evento(ev)?;
        }
        pasta.passo(&saida.linha)?;
        for r in &saida.rastro {
            pasta.rastro(&cen, r)?;
        }
        if let Some(v) = saida.violacao {
            eprintln!("{v}");
            desfecho = Desfecho::Falhou;
            motivo = Some(v);
            violou_invariante = true;
            break;
        }
        if ultimo_flush.elapsed() >= intervalo {
            pasta.descarregar(
                &registro::resumo(&cen, &laco, Desfecho::EmAndamento, None, inicio.elapsed().as_secs_f64()),
                &registro::tomadas_csv(&cen, &laco),
            )?;
            ultimo_flush = Instant::now();
        }
    }
    let parede_s = inicio.elapsed().as_secs_f64();
    drop(monitor);

    if desfecho == Desfecho::Completou {
        if let Some(r) = o.plantar {
            if !laco.plantada {
                desfecho = Desfecho::Falhou;
                motivo = Some(format!("regressão não plantada: {}", r.nome()));
            } else {
                // Plantou e a conferencia nao pegou: defeito do simulador, nunca verde.
                desfecho = Desfecho::Falhou;
                motivo = Some(format!(
                    "regressão {} plantada e NÃO pega pela conferência: defeito do simulador",
                    r.nome()
                ));
            }
        }
    }

    // As expectativas foram escritas para a rodada inteira: so valem numa rodada que chegou ao
    // fim. Avaliadas sobre um parcial, acusariam falhas que nao existem.
    let expectativas =
        (!recorte && desfecho == Desfecho::Completou).then(|| laco::avaliar_expectativas(&caso, &laco));
    if desfecho == Desfecho::Completou {
        if let Some(f) = expectativas.as_ref().and_then(|l| l.iter().find(|e| !e.passou)) {
            let m = format!("expectativa {} falhou: {}", f.descricao, f.obtido);
            eprintln!("FALHOU {m}");
            desfecho = Desfecho::Falhou;
            motivo = Some(m);
        }
    }

    if o.determinismo && matches!(desfecho, Desfecho::Completou | Desfecho::ParouNoOrcamento) {
        eprintln!("determinismo: rodando de novo ({} passos, contra os digests gravados em disco)", milhar(laco.passo));
        let gravados = digests.as_mut().expect("--determinismo grava os digests");
        gravados.reler()?;
        let monitor2 = Monitor::iniciar(o.progresso_s, laco.passo, n, Projecao::Nenhuma);
        let mut segunda = Laco::novo(&cen, &base, None, false, Some(monitor2.estado()))?;
        segunda.realimentar = o.realimentar;
        while segunda.passo < laco.passo {
            segunda.passo()?;
            let k = segunda.passo - 1;
            if gravados.proximo()? != segunda.ultimo_digest {
                let m = format!("determinismo: primeiro passo divergente {}", milhar(k));
                eprintln!("FALHOU {m}");
                desfecho = Desfecho::Falhou;
                motivo = Some(m);
                break;
            }
        }
    }

    // Apaga o arquivo temporario dos digests.
    drop(digests);

    let resumo = registro::resumo(&cen, &laco, desfecho, motivo.clone(), parede_s);
    pasta.descarregar(&resumo, &registro::tomadas_csv(&cen, &laco))?;
    let relatorio = registro::relatorio(&registro::DadosRelatorio {
        caso: &caso,
        cen: &cen,
        params: &params,
        resumo: &resumo,
        laco: &laco,
        avisos: &avisos,
        expectativas: expectativas.as_deref(),
        recorte,
    });
    pasta.escrever("relatorio.md", &relatorio)?;

    if !o.quieto {
        imprimir_resumo(&cen, &laco, &resumo, expectativas.as_deref(), recorte);
        println!("pasta: {}", pasta.dir.display());
    }

    Ok(Resultado {
        desfecho,
        motivo,
        pasta: pasta.dir.clone(),
        recorte,
        violou_invariante,
        expectativas: expectativas.map(|l| (l.iter().filter(|e| e.passou).count(), l.len())),
    })
}

fn imprimir_resumo(
    cen: &Cenario,
    laco: &Laco,
    r: &registro::Resumo,
    expectativas: Option<&[laco::Expectativa]>,
    recorte: bool,
) {
    const MAX_LINHAS: usize = 20;
    println!();
    println!("{:<20} {:>7} {:>10} {:>12} {:>10} {:>10} {:>10}", "tomada", "cortes", "passos_off", "religamentos", "off_h", "std_h", "on_h");
    let n = cen.tomadas.len();
    let mut ordem: Vec<usize> = (0..n).collect();
    if n > MAX_LINHAS {
        ordem.sort_by(|&a, &b| laco.estados[b].acc.cortes.cmp(&laco.estados[a].acc.cortes).then(a.cmp(&b)));
    }
    for &i in ordem.iter().take(MAX_LINHAS) {
        let a = &laco.estados[i].acc;
        println!(
            "{:<20} {:>7} {:>10} {:>12} {:>10} {:>10} {:>10}",
            cen.tomadas[i].nome,
            milhar(a.cortes),
            milhar(a.passos_off),
            milhar(a.religamentos),
            virgula(a.tempo_off_s as f64 / 3600.0, 2),
            virgula(a.tempo_std_s as f64 / 3600.0, 2),
            virgula(a.tempo_on_s as f64 / 3600.0, 2)
        );
    }
    if n > MAX_LINHAS {
        println!("... mais {} tomadas em tomadas.csv (acima, as {MAX_LINHAS} com mais cortes)", n - MAX_LINHAS);
    }

    let mut por_tipo: Vec<(String, u64)> = Vec::new();
    for e in &laco.estados {
        for (tipo, qtd) in [("penalidade_uso", e.acc.penalidades_uso), ("penalidade_age", e.acc.penalidades_age)] {
            if qtd > 0 {
                match por_tipo.iter_mut().find(|(t, _)| t == tipo) {
                    Some((_, q)) => *q += qtd,
                    None => por_tipo.push((tipo.to_string(), qtd)),
                }
            }
        }
    }
    println!();
    println!(
        "casa: base {} Wh, com o sistema {} Wh, economia {} Wh",
        virgula(r.energia_base_wh, 1),
        virgula(r.energia_com_sistema_wh, 1),
        virgula(r.economia_wh, 1)
    );
    println!(
        "cortes {}, passos acima do teto {} (pior excesso {} W), penalidades: {}",
        milhar(r.cortes),
        milhar(r.passos_acima_do_teto),
        virgula(r.pior_excesso_w, 1),
        if por_tipo.is_empty() {
            "nenhuma".to_string()
        } else {
            por_tipo.iter().map(|(t, q)| format!("{t}={}", milhar(*q))).collect::<Vec<_>>().join(" ")
        }
    );
    println!(
        "custo: {} binárias por solve, decidir mediana {} us / p90 {} us / max {} us, {} solves, parede {} (tempo de PC, não do Xtensa)",
        r.custo.variaveis_binarias_por_solve,
        milhar(r.custo.decidir_mediana_us as u64),
        milhar(r.custo.decidir_p90_us as u64),
        milhar(r.custo.decidir_max_us as u64),
        milhar(r.custo.solves),
        duracao_humana(r.custo.parede_s)
    );
    match expectativas {
        None => println!(
            "expectativas: não avaliadas ({})",
            if recorte { "recorte" } else { "a rodada não chegou ao fim" }
        ),
        Some(l) if !l.is_empty() => {
            for e in l {
                println!("expectativa {}: {} ({})", e.descricao, if e.passou { "passou" } else { "FALHOU" }, e.obtido);
            }
        }
        _ => {}
    }
    println!("desfecho: {}{}", r.desfecho, r.motivo.as_ref().map(|m| format!(" - {m}")).unwrap_or_default());
    println!("digest: {}", r.digest);
}

/// `verificar <pasta-ou-caso>` (`contracts/cli.md`, SC-005, SC-010).
fn verificar(alvo: &Path, saida: &Path, progresso_s: u64) -> Result<i32> {
    let mut casos: Vec<PathBuf> = if alvo.is_dir() {
        std::fs::read_dir(alvo)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "toml"))
            .collect()
    } else {
        vec![alvo.to_path_buf()]
    };
    casos.sort();
    if casos.is_empty() {
        return Err(invalido(format!("{}: nenhum .toml", alvo.display())));
    }

    let inicio = Instant::now();
    let mut tudo_ok = true;
    let mut pegas_total = 0usize;
    let mut linhas = Vec::new();
    for caminho in &casos {
        let rotulo = caminho.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let caso = match caso::carregar(caminho) {
            Ok(c) => c,
            Err(e) => {
                tudo_ok = false;
                linhas.push(format!("{rotulo}: REPROVADO - caso inválido: {e}"));
                continue;
            }
        };
        if !caso.base_arquivo.exists() && caso.tem_perfis() {
            eprintln!("{rotulo}: gerando a base {}", caso.base_arquivo.display());
            if let Err(e) = gerar_base::gerar(&caso, false) {
                tudo_ok = false;
                linhas.push(format!("{rotulo}: REPROVADO - gerar-base: {e}"));
                continue;
            }
        }
        let ate = (caso.duracao_s > VERIFICAR_INTEIRO_ATE_S).then(|| VERIFICAR_RECORTE.to_string());
        let opcoes = |rot: &str, plantar: Option<Regra>, determinismo: bool| Opcoes {
            caso: caminho.clone(),
            rotulo: Some(rot.to_string()),
            rastro: false,
            orcamento: None,
            ate: ate.clone(),
            determinismo,
            plantar,
            realimentar: false,
            saida: saida.to_path_buf(),
            progresso_s,
            quieto: true,
        };

        eprintln!("{rotulo}: rodada principal{}", if ate.is_some() { " (recorte de 6 h)" } else { "" });
        let mut partes = Vec::new();
        let mut ok_caso = true;
        match executar(&opcoes("verificar", None, true)) {
            Ok(r) => {
                let exp = match (r.recorte, r.expectativas) {
                    (true, _) => "expectativas: não avaliadas (recorte)".to_string(),
                    (false, Some((p, t))) => format!("expectativas {p}/{t}"),
                    (false, None) => "sem expectativas".to_string(),
                };
                if r.desfecho == Desfecho::Completou {
                    partes.push(format!("principal completou (determinismo ok, {exp})"));
                } else {
                    ok_caso = false;
                    partes.push(format!(
                        "principal {} ({}) em {}",
                        r.desfecho.as_str(),
                        r.motivo.unwrap_or_default(),
                        r.pasta.display()
                    ));
                }
            }
            Err(e) => {
                ok_caso = false;
                partes.push(format!("principal: erro {e}"));
            }
        }
        for regra in [Regra::R4, Regra::R3, Regra::R5] {
            eprintln!("{rotulo}: plantando {}", regra.nome());
            match executar(&opcoes(&format!("plantar-{}", regra.nome()), Some(regra), false)) {
                Ok(r) if r.violou_invariante => {
                    pegas_total += 1;
                    partes.push(format!("{} pega", regra.nome()));
                }
                Ok(r) if r.motivo.as_deref().is_some_and(|m| m.starts_with("regressão não plantada")) => {
                    partes.push(format!("{} não plantável", regra.nome()));
                }
                Ok(r) => {
                    ok_caso = false;
                    partes.push(format!("{} REPROVADA ({}: {})", regra.nome(), r.desfecho.as_str(), r.motivo.unwrap_or_default()));
                }
                Err(e) => {
                    ok_caso = false;
                    partes.push(format!("{} erro {e}", regra.nome()));
                }
            }
        }
        tudo_ok &= ok_caso;
        let linha = format!("{rotulo}: {} | {}", if ok_caso { "ok" } else { "REPROVADO" }, partes.join(" | "));
        eprintln!("{linha}");
        linhas.push(linha);
    }

    println!();
    for l in &linhas {
        println!("{l}");
    }
    let sc010 = pegas_total >= 1;
    if !sc010 {
        tudo_ok = false;
    }
    println!(
        "total: {} casos, {} regressões plantadas e pegas{}, {} em {}",
        casos.len(),
        pegas_total,
        if sc010 { "" } else { " (SC-010 exige ao menos uma)" },
        if tudo_ok { "TUDO PASSOU" } else { "REPROVADO" },
        duracao_humana(inicio.elapsed().as_secs_f64())
    );
    Ok(if tudo_ok { 0 } else { 1 })
}
