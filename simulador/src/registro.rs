//! A pasta da rodada (`contracts/rodada.md`, research D9): gravacao incremental, escrita
//! atomica dos arquivos reescritos e o `relatorio.md`.

use std::fmt::Write as _;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::base::Base;
use crate::caso::Caso;
use crate::firmware;
use crate::formato::{csv, duracao_humana, milhar, num, virgula};
use crate::laco::{
    estatisticas, Cenario, Desfecho, Evento, Expectativa, Laco, LinhaPasso, LinhaRastro, CICLO_S, MAX_CORTES_CRITICAS_LISTADOS,
};

/// Acima disto, um CSV anexado vai ao disco antes do proximo descarregamento.
const LIMITE_ANEXO: usize = 1 << 20;

pub const FRASE_MODO: &str = "A base foi reproduzida como está: a decisão do solver não altera o consumo do passo seguinte. A economia abaixo é uma estimativa (a energia da base nos intervalos decididos OFF).";
pub const MODO_REALIMENTA: &str = "realimenta";
const FRASE_REALIMENTA: &str = "Com realimentação: a tomada decidida OFF lê 0 W no passo seguinte, como a placa com o relé aberto. A base é o que o aparelho puxaria se estivesse energizado.";

pub fn frase_modo(modo: &str) -> &'static str {
    if modo == MODO_REALIMENTA { FRASE_REALIMENTA } else { FRASE_MODO }
}

pub fn aviso_passo(passo_s: i64) -> Option<String> {
    (passo_s > CICLO_S).then(|| {
        format!(
            "Aviso: o passo da simulação ({passo_s} s) é maior que o ciclo real de {CICLO_S} s. Dois efeitos: a ociosidade é percebida mais tarde, porque o F_i só vira no primeiro passo que cruza o limite; e a calibração de standby e de uso, que avança um pouco a cada ciclo, converge {}x mais devagar em tempo simulado.",
            virgula(passo_s as f64 / CICLO_S as f64, 1)
        )
    })
}

#[derive(Serialize)]
pub struct Parametros {
    pub caso: String,
    pub caso_sha256: String,
    pub base_arquivo: String,
    pub base_sha256: String,
    pub base_passo_s: i64,
    pub fonte_firmware_sha256: String,
    pub simulador_versao: &'static str,
    pub modo: &'static str,
    pub passo_s: i64,
    pub passo_maior_que_ciclo: bool,
    pub orcamento_s: Option<i64>,
    pub n_tomadas: usize,
    pub passos_total: u64,
    pub solves_previstos: u64,
    pub inicio: String,
    pub rotulo: Option<String>,
    pub iniciada_em: String,
}

impl Parametros {
    pub fn novo(caso: &Caso, base: &Base, cen: &Cenario, orcamento_s: Option<i64>, rotulo: Option<String>) -> Self {
        use sha2::{Digest, Sha256};
        Parametros {
            caso: caso.nome.clone(),
            caso_sha256: firmware::hex(&Sha256::digest(&caso.bytes)),
            base_arquivo: base.arquivo.display().to_string().replace('\\', "/"),
            base_sha256: base.sha256.clone(),
            base_passo_s: base.passo_s,
            fonte_firmware_sha256: firmware::fonte_firmware_sha256(),
            simulador_versao: env!("CARGO_PKG_VERSION"),
            modo: "reproduz-base",
            passo_s: cen.passo_s,
            passo_maior_que_ciclo: cen.passo_s > CICLO_S,
            orcamento_s,
            n_tomadas: cen.tomadas.len(),
            passos_total: cen.passos_total,
            solves_previstos: cen.passos_total * cen.tomadas.len() as u64,
            inicio: cen.inicio.format("%Y-%m-%dT%H:%M").to_string(),
            rotulo,
            iniciada_em: chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
        }
    }
}

#[derive(Serialize)]
pub struct PassoLentoJson {
    pub passo: u64,
    pub instante: String,
    pub duracao_ms: u64,
}

#[derive(Serialize)]
pub struct Custo {
    pub variaveis_binarias_por_solve: u64,
    pub decidir_mediana_us: u32,
    pub decidir_p90_us: u32,
    pub decidir_max_us: u32,
    pub solves: u64,
    pub total_s: f64,
    pub parede_s: f64,
}

#[derive(Serialize)]
pub struct Resumo {
    pub desfecho: &'static str,
    pub motivo: Option<String>,
    pub ultimo_passo: u64,
    pub energia_base_wh: f64,
    pub energia_com_sistema_wh: f64,
    pub economia_wh: f64,
    pub cortes: u64,
    pub passos_off: u64,
    pub passos_acima_do_teto: u64,
    pub pior_excesso_w: f64,
    pub passo_mais_lento: Option<PassoLentoJson>,
    pub custo: Custo,
    pub digest: String,
}

pub fn resumo(cen: &Cenario, laco: &Laco, desfecho: Desfecho, motivo: Option<String>, parede_s: f64) -> Resumo {
    let tot = laco.totais();
    let (mediana, p90, max) = estatisticas(&laco.duracoes);
    Resumo {
        desfecho: desfecho.as_str(),
        motivo,
        ultimo_passo: laco.passo,
        energia_base_wh: tot.energia_base_wh,
        energia_com_sistema_wh: tot.energia_com_sistema_wh,
        economia_wh: tot.economia_wh(),
        cortes: tot.cortes,
        passos_off: tot.passos_off,
        passos_acima_do_teto: laco.passos_acima_do_teto,
        pior_excesso_w: laco.pior_excesso_w,
        passo_mais_lento: laco
            .passo_mais_lento
            .as_ref()
            .map(|p| PassoLentoJson { passo: p.passo, instante: p.instante.clone(), duracao_ms: p.duracao_ms }),
        custo: Custo {
            variaveis_binarias_por_solve: 3 * cen.tomadas.len() as u64,
            decidir_mediana_us: mediana,
            decidir_p90_us: p90,
            decidir_max_us: max,
            solves: laco.passo * cen.tomadas.len() as u64,
            total_s: laco.decidir_total_us as f64 / 1e6,
            parede_s,
        },
        digest: laco.digest_hex(),
    }
}

pub fn tomadas_csv(cen: &Cenario, laco: &Laco) -> String {
    let mut s = String::from(
        "tomada,grupo,coluna,energia_base_wh,energia_com_sistema_wh,economia_wh,critica,cortes,passos_off,religamentos,tempo_off_s,tempo_std_s,tempo_on_s,penalidades_uso,penalidades_age,botoes_usar_agora,decidir_mediana_us\n",
    );
    for (t, e) in cen.tomadas.iter().zip(&laco.estados) {
        let a = &e.acc;
        let mediana = a.duracoes.percentil(0.5);
        let _ = writeln!(
            s,
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            csv(&t.nome),
            csv(&t.grupo),
            csv(&t.coluna),
            num(a.energia_base_wh, 3),
            num(a.energia_com_sistema_wh, 3),
            num(a.economia_wh(), 3),
            t.config.is_critical,
            a.cortes,
            a.passos_off,
            a.religamentos,
            a.tempo_off_s,
            a.tempo_std_s,
            a.tempo_on_s,
            a.penalidades_uso,
            a.penalidades_age,
            a.botoes,
            mediana
        );
    }
    s
}

pub struct Pasta {
    pub dir: PathBuf,
    eventos: Anexo,
    passos: Anexo,
    rastro: Option<Anexo>,
}

/// CSV anexado. As linhas ficam em memoria e so vao ao disco inteiras, no descarregamento:
/// um `BufWriter` descarregaria sozinho a cada 8 KB, e uma interrupcao deixaria meia linha.
struct Anexo {
    arquivo: File,
    buf: Vec<u8>,
}

impl Anexo {
    /// Manda o buffer ao disco quando passa de `LIMITE_ANEXO`. Ele so tem linhas inteiras,
    /// entao a garantia de nunca deixar meia linha continua, e a memoria nao cresce numa
    /// rodada rapida que gera muitas linhas entre dois descarregamentos.
    fn aliviar(&mut self) -> Result<()> {
        if self.buf.len() >= LIMITE_ANEXO {
            self.arquivo.write_all(&self.buf)?;
            self.buf.clear();
        }
        Ok(())
    }

    fn descarregar(&mut self) -> Result<()> {
        if !self.buf.is_empty() {
            self.arquivo.write_all(&self.buf)?;
            self.buf.clear();
        }
        self.arquivo.flush()?;
        Ok(())
    }
}

fn criar_csv(dir: &Path, nome: &str, cabecalho: &str) -> Result<Anexo> {
    let caminho = dir.join(nome);
    let mut arquivo = File::create(&caminho).with_context(|| format!("criando {}", caminho.display()))?;
    writeln!(arquivo, "{cabecalho}")?;
    Ok(Anexo { arquivo, buf: Vec::new() })
}

impl Pasta {
    /// `<saida>/<caso>/<AAAAMMDD-HHMMSS>[-<rotulo>]/`, com `-2`, `-3`... em colisao: uma
    /// rodada nunca sobrescreve outra (FR-024b).
    pub fn criar(saida: &Path, caso: &Caso, params: &Parametros, rastro: bool) -> Result<Pasta> {
        let pai = saida.join(&caso.nome);
        std::fs::create_dir_all(&pai).with_context(|| format!("criando {}", pai.display()))?;
        let mut base_nome = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
        if let Some(r) = &params.rotulo {
            base_nome.push('-');
            base_nome.push_str(r);
        }
        let mut dir = pai.join(&base_nome);
        let mut sufixo = 2;
        loop {
            match std::fs::create_dir(&dir) {
                Ok(()) => break,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    dir = pai.join(format!("{base_nome}-{sufixo}"));
                    sufixo += 1;
                }
                Err(e) => return Err(e).with_context(|| format!("criando {}", dir.display())),
            }
        }
        std::fs::write(dir.join("caso.toml"), &caso.bytes).context("copiando caso.toml")?;
        std::fs::write(dir.join("parametros.json"), serde_json::to_string_pretty(params)? + "\n")
            .context("gravando parametros.json")?;
        let eventos = criar_csv(&dir, "eventos.csv", "passo,t_s,instante,tomada,tipo,severidade,titulo,detalhes")?;
        let passos = criar_csv(
            &dir,
            "passos.csv",
            "passo,t_s,decidir_total_us,decidir_max_us,tomadas_off,consumo_modelo_w,teto_w,acima_do_teto",
        )?;
        let rastro = if rastro {
            Some(criar_csv(&dir, "rastro.csv", "passo,t_s,tomada,base_w,u_i,f_i,estado,cstd,con,decidir_us")?)
        } else {
            None
        };
        Ok(Pasta { dir, eventos, passos, rastro })
    }

    pub fn evento(&mut self, e: &Evento) -> Result<()> {
        writeln!(
            self.eventos.buf,
            "{},{},{},{},{},{},{},{}",
            e.passo,
            e.t_s,
            e.instante,
            csv(&e.tomada),
            csv(&e.tipo),
            csv(&e.severidade),
            csv(&e.titulo),
            csv(&e.detalhes)
        )?;
        self.eventos.aliviar()
    }

    pub fn passo(&mut self, l: &LinhaPasso) -> Result<()> {
        writeln!(
            self.passos.buf,
            "{},{},{},{},{},{},{},{}",
            l.passo,
            l.t_s,
            l.decidir_total_us,
            l.decidir_max_us,
            l.tomadas_off,
            num(l.consumo_modelo_w, 3),
            num(l.teto_w, 3),
            l.acima_do_teto as u8
        )?;
        self.passos.aliviar()
    }

    pub fn rastro(&mut self, cen: &Cenario, r: &LinhaRastro) -> Result<()> {
        if let Some(w) = &mut self.rastro {
            writeln!(
                w.buf,
                "{},{},{},{},{},{},{},{},{},{}",
                r.passo,
                r.t_s,
                csv(&cen.tomadas[r.tomada].nome),
                num(r.base_w as f64, 3),
                r.u_i as u8,
                r.f_i as u8,
                r.estado.as_str(),
                num(r.cstd as f64, 3),
                num(r.con as f64, 3),
                r.decidir_us
            )?;
            w.aliviar()?;
        }
        Ok(())
    }

    /// Descarrega os anexados e reescreve `resumo.json` e `tomadas.csv` por arquivo
    /// temporario + `rename`, para nunca ficarem pela metade (FR-024a).
    pub fn descarregar(&mut self, resumo: &Resumo, tomadas: &str) -> Result<()> {
        self.eventos.descarregar()?;
        self.passos.descarregar()?;
        if let Some(w) = &mut self.rastro {
            w.descarregar()?;
        }
        self.escrever("resumo.json", &(serde_json::to_string_pretty(resumo)? + "\n"))?;
        self.escrever("tomadas.csv", tomadas)
    }

    pub fn escrever(&self, nome: &str, conteudo: &str) -> Result<()> {
        let tmp = self.dir.join(format!(".{nome}.tmp"));
        std::fs::write(&tmp, conteudo).with_context(|| format!("gravando {}", tmp.display()))?;
        std::fs::rename(&tmp, self.dir.join(nome)).with_context(|| format!("renomeando {nome}"))?;
        Ok(())
    }
}

/// Digest de cada passo em disco, para o `--determinismo` apontar o primeiro passo
/// divergente sem guardar um por passo em memoria. Arquivo oculto na pasta da rodada,
/// apagado no `Drop`; uma rodada morta no meio pode deixa-lo para tras.
pub struct Digests {
    caminho: PathBuf,
    escrita: Option<BufWriter<File>>,
    leitura: Option<BufReader<File>>,
}

impl Digests {
    pub fn criar(dir: &Path) -> Result<Digests> {
        let caminho = dir.join(".digests-determinismo.tmp");
        let arquivo = File::create(&caminho).with_context(|| format!("criando {}", caminho.display()))?;
        Ok(Digests { caminho, escrita: Some(BufWriter::new(arquivo)), leitura: None })
    }

    pub fn gravar(&mut self, d: &[u8; 32]) -> Result<()> {
        self.escrita.as_mut().context("digests ja relidos")?.write_all(d)?;
        Ok(())
    }

    /// Fecha a escrita e volta ao primeiro digest.
    pub fn reler(&mut self) -> Result<()> {
        if let Some(mut w) = self.escrita.take() {
            w.flush()?;
        }
        let arquivo = File::open(&self.caminho).with_context(|| format!("abrindo {}", self.caminho.display()))?;
        self.leitura = Some(BufReader::new(arquivo));
        Ok(())
    }

    pub fn proximo(&mut self) -> Result<[u8; 32]> {
        let mut d = [0u8; 32];
        self.leitura.as_mut().context("digests nao relidos")?.read_exact(&mut d).context("lendo digests")?;
        Ok(d)
    }
}

impl Drop for Digests {
    fn drop(&mut self) {
        // Fecha antes: no Windows um arquivo aberto nao se apaga.
        self.escrita.take();
        self.leitura.take();
        let _ = std::fs::remove_file(&self.caminho);
    }
}

/// Tudo o que o relatorio le.
pub struct DadosRelatorio<'a> {
    pub caso: &'a Caso,
    pub cen: &'a Cenario,
    pub params: &'a Parametros,
    pub resumo: &'a Resumo,
    pub laco: &'a Laco<'a>,
    pub avisos: &'a [String],
    pub expectativas: Option<&'a [Expectativa]>,
    pub recorte: bool,
}

fn kwh(wh: f64) -> String {
    virgula(wh / 1000.0, 3)
}

/// `relatorio.md`: para gente, com virgula decimal e unidade no cabecalho (FR-023).
pub fn relatorio(d: &DadosRelatorio) -> String {
    let r = d.resumo;
    let cen = d.cen;
    let n = cen.tomadas.len();
    let mut s = String::new();
    let _ = writeln!(s, "# Rodada: {}\n", d.caso.nome);
    let desfecho = match &r.motivo {
        Some(m) => format!("**Desfecho: {}** - {}", r.desfecho, m),
        None => format!("**Desfecho: {}**", r.desfecho),
    };
    let _ = writeln!(s, "{desfecho}\n");
    let _ = writeln!(s, "> {}\n", frase_modo(d.params.modo));
    if let Some(a) = aviso_passo(cen.passo_s) {
        let _ = writeln!(s, "> {a}\n");
    }
    if !d.caso.descricao.is_empty() {
        let _ = writeln!(s, "{}\n", d.caso.descricao);
    }

    let _ = writeln!(s, "## Parâmetros\n");
    let _ = writeln!(s, "| Item | Valor |\n|---|---|");
    let _ = writeln!(s, "| Tomadas | {} |", n);
    let _ = writeln!(s, "| Início simulado | {} |", d.params.inicio);
    let _ = writeln!(s, "| Passo (s) | {} |", cen.passo_s);
    let _ = writeln!(
        s,
        "| Passos concluídos / previstos | {} / {}{} |",
        milhar(r.ultimo_passo),
        milhar(cen.passos_total),
        if d.recorte { " (recorte com --ate)" } else { "" }
    );
    let _ = writeln!(s, "| P_max (W) | {} |", virgula(cen.p_max_w as f64, 1));
    let _ = writeln!(s, "| Carga fixa (W) | {} |", virgula(cen.carga_fixa_w as f64, 1));
    let _ = writeln!(s, "| Base | `{}` |", d.params.base_arquivo);
    let _ = writeln!(s, "| SHA-256 do caso | `{}` |", d.params.caso_sha256);
    let _ = writeln!(s, "| SHA-256 da base | `{}` |", d.params.base_sha256);
    let _ = writeln!(s, "| SHA-256 dos fontes do firmware | `{}` |", d.params.fonte_firmware_sha256);
    let _ = writeln!(s, "| Digest de determinismo | `{}` |\n", r.digest);

    if !d.avisos.is_empty() {
        let _ = writeln!(s, "## Avisos de validação\n");
        for a in d.avisos {
            let _ = writeln!(s, "- {a}");
        }
        let _ = writeln!(s);
    }

    let _ = writeln!(s, "## Energia da casa\n");
    let _ = writeln!(s, "| Energia da base (kWh) | Energia estimada com o sistema (kWh) | Economia estimada (kWh) | Economia (%) |");
    let _ = writeln!(s, "|---:|---:|---:|---:|");
    let pct = if r.energia_base_wh > 0.0 { r.economia_wh / r.energia_base_wh * 100.0 } else { 0.0 };
    let _ = writeln!(
        s,
        "| {} | {} | {} | {} |\n",
        kwh(r.energia_base_wh),
        kwh(r.energia_com_sistema_wh),
        kwh(r.economia_wh),
        virgula(pct, 2)
    );

    let _ = writeln!(s, "## Por grupo\n");
    let _ = writeln!(s, "| Grupo | Tomadas | Energia da base (kWh) | Com o sistema (kWh) | Economia (kWh) | Cortes | Religamentos | Tempo OFF (h) |");
    let _ = writeln!(s, "|---|---:|---:|---:|---:|---:|---:|---:|");
    let mut grupos: Vec<&str> = Vec::new();
    for t in &cen.tomadas {
        if !grupos.contains(&t.grupo.as_str()) {
            grupos.push(&t.grupo);
        }
    }
    for g in grupos {
        let (mut qtd, mut base, mut com, mut cortes, mut relig, mut off) = (0, 0.0, 0.0, 0, 0, 0i64);
        for (t, e) in cen.tomadas.iter().zip(&d.laco.estados) {
            if t.grupo == g {
                qtd += 1;
                base += e.acc.energia_base_wh;
                com += e.acc.energia_com_sistema_wh;
                cortes += e.acc.cortes;
                relig += e.acc.religamentos;
                off += e.acc.tempo_off_s;
            }
        }
        let _ = writeln!(
            s,
            "| {g} | {qtd} | {} | {} | {} | {} | {} | {} |",
            kwh(base),
            kwh(com),
            kwh(base - com),
            milhar(cortes),
            milhar(relig),
            virgula(off as f64 / 3600.0, 2)
        );
    }
    let _ = writeln!(s);

    let _ = writeln!(s, "## Tomadas de maior economia\n");
    let _ = writeln!(s, "| Tomada | Economia (Wh) | Cortes | Tempo OFF (h) | Tempo STD (h) | Tempo ON (h) | Penalidades de uso | Penalidades de agendamento |");
    let _ = writeln!(s, "|---|---:|---:|---:|---:|---:|---:|---:|");
    let mut ordem: Vec<usize> = (0..n).collect();
    ordem.sort_by(|&a, &b| {
        d.laco.estados[b].acc.economia_wh().total_cmp(&d.laco.estados[a].acc.economia_wh()).then(a.cmp(&b))
    });
    for &i in ordem.iter().take(10) {
        let a = &d.laco.estados[i].acc;
        let _ = writeln!(
            s,
            "| {} | {} | {} | {} | {} | {} | {} | {} |",
            cen.tomadas[i].nome,
            virgula(a.economia_wh(), 2),
            milhar(a.cortes),
            virgula(a.tempo_off_s as f64 / 3600.0, 2),
            virgula(a.tempo_std_s as f64 / 3600.0, 2),
            virgula(a.tempo_on_s as f64 / 3600.0, 2),
            milhar(a.penalidades_uso),
            milhar(a.penalidades_age)
        );
    }
    let _ = writeln!(s);

    let c = &r.custo;
    let _ = writeln!(s, "## Custo de decidir\n");
    let _ = writeln!(s, "| Item | Valor |\n|---|---|");
    let _ = writeln!(s, "| Tamanho do problema (variáveis binárias por solve, 3N) | {} |", milhar(c.variaveis_binarias_por_solve));
    let _ = writeln!(s, "| Solves por passo | {} |", milhar(n as u64));
    let _ = writeln!(s, "| Solves na rodada | {} |", milhar(c.solves));
    let _ = writeln!(s, "| Mediana de decidir (us) | {} |", milhar(c.decidir_mediana_us as u64));
    let _ = writeln!(s, "| p90 de decidir (us) | {} |", milhar(c.decidir_p90_us as u64));
    let _ = writeln!(s, "| Máximo de decidir (us) | {} |", milhar(c.decidir_max_us as u64));
    let _ = writeln!(s, "| Soma dos decidir (s) | {} |", virgula(c.total_s, 3));
    let _ = writeln!(s, "| Tempo de parede da rodada (s) | {} |", virgula(c.parede_s, 3));
    if let Some(p) = &r.passo_mais_lento {
        let _ = writeln!(s, "| Passo mais lento | {} ({}), {} ms |", milhar(p.passo), p.instante, milhar(p.duracao_ms));
    }
    let _ = writeln!(
        s,
        "\nA medição é o `decidir` inteiro, cronometrado de fora: derivação de U_i e F_i, calibração EMA, montagem do `SolverInput`, ordenação por MAC, o solve (branch-and-bound sobre 3N binárias) e a aplicação do resultado. O solve domina, mas o número não é só o solve. É tempo de PC (x86_64, opt-level 3) e não traduz o tempo no Xtensa a 240 MHz. Mediana e p90 saem de um histograma de memória fixa: exatos até 63 us e, acima disso, com erro de no máximo 0,8%. O máximo é exato.\n"
    );

    let _ = writeln!(s, "## Críticas cortadas\n");
    if d.laco.cortes_criticas.is_empty() {
        let _ = writeln!(s, "Nenhuma tomada crítica foi cortada.\n");
    } else {
        let _ = writeln!(s, "Com W_i = M, uma crítica só é cortada quando o teto obriga.\n");
        let _ = writeln!(s, "| Passo | Instante | Tomada | Teto disponível (W) | Consumo do modelo no passo (W) |");
        let _ = writeln!(s, "|---:|---|---|---:|---:|");
        for cc in &d.laco.cortes_criticas {
            let _ = writeln!(
                s,
                "| {} | {} | {} | {} | {} |",
                milhar(cc.passo),
                cc.instante,
                cc.tomada,
                virgula(cc.teto_w, 1),
                virgula(cc.consumo_modelo_w, 1)
            );
        }
        let fora = d.laco.cortes_criticas_total.saturating_sub(d.laco.cortes_criticas.len() as u64);
        if fora > 0 {
            let _ = writeln!(
                s,
                "\nE mais {} cortes de críticas fora da tabela: ela guarda os primeiros {} ({} no total).",
                milhar(fora),
                milhar(MAX_CORTES_CRITICAS_LISTADOS as u64),
                milhar(d.laco.cortes_criticas_total)
            );
        }
        let _ = writeln!(s);
    }

    let _ = writeln!(s, "## Defasagem de coordenação\n");
    let _ = writeln!(s, "| Passos acima do teto | Pior excesso (W) |\n|---:|---:|");
    let _ = writeln!(s, "| {} | {} |\n", milhar(r.passos_acima_do_teto), virgula(r.pior_excesso_w, 2));
    let _ = writeln!(
        s,
        "Cada tomada decide vendo as vizinhas pelo anúncio do passo anterior, como na malha ESP-NOW. A soma dos consumos do modelo (C_STD ou C_ON de cada estado decidido) pode passar do teto disponível num passo sem que nenhum solve tenha violado a R2. É métrica, não falha (FR-019a).\n"
    );

    let _ = writeln!(s, "## O que não foi conferido\n");
    let _ = writeln!(
        s,
        "- A R2 dentro de cada solve e a integralidade das variáveis x: a interface do `decidir` devolve só o estado da própria tomada (FR-019b).\n- O valor da objetivo Z fora dos passos que emitem evento: ele só aparece nos eventos do firmware.\n- Conferido em todo passo: R1 (uma decisão por tomada), R4 (nunca STD com F_i), R3 (OFF com uso ativo se e somente se há penalidade de uso) e R5 (ON fora do horário se e somente se há penalidade de agendamento).\n"
    );

    let _ = writeln!(s, "## Expectativas\n");
    match d.expectativas {
        None if d.recorte => {
            let _ = writeln!(s, "Não avaliadas (recorte): as expectativas foram escritas para a duração inteira do caso.");
        }
        None => {
            let _ = writeln!(s, "Não avaliadas: a rodada não chegou ao fim, e as expectativas valem para a rodada inteira.");
        }
        Some([]) => {
            let _ = writeln!(s, "O caso não declara expectativas.");
        }
        Some(lista) => {
            let _ = writeln!(s, "| Expectativa | Resultado | Obtido |\n|---|---|---|");
            for e in lista {
                let _ = writeln!(s, "| {} | {} | {} |", e.descricao, if e.passou { "passou" } else { "falhou" }, e.obtido);
            }
        }
    }
    let _ = writeln!(s, "\nDuração da rodada: {}.", duracao_humana(c.parede_s));
    s
}
