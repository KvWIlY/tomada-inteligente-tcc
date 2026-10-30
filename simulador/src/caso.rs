//! Caso TOML: leitura, validacao e expansao dos grupos em tomadas (data-model §2,
//! `contracts/caso.md`). A `Tomada` montada aqui e a mesma `persistencia::config::Tomada`
//! do firmware, em memoria, nunca lida de disco.

use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::NaiveDateTime;
use serde::Deserialize;

use crate::persistencia::config::Tomada;

/// Entrada recusada antes de rodar: caso, base ou opcao invalidos (codigo de saida 2).
#[derive(Debug)]
pub struct Invalido(pub String);

impl fmt::Display for Invalido {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Invalido {}

pub fn invalido(msg: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(Invalido(msg.into()))
}

/// O MAC sintetico usa dois bytes para o indice.
pub const MAX_TOMADAS: usize = 65_536;
const INICIO_PADRAO: &str = "2026-01-05T00:00";

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum DuracaoToml {
    Segundos(i64),
    Texto(String),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CasoToml {
    nome: Option<String>,
    #[serde(default)]
    descricao: String,
    inicio: Option<String>,
    duracao: DuracaoToml,
    passo_s: Option<i64>,
    base: BaseToml,
    casa: CasaToml,
    #[serde(default)]
    limites: LimitesToml,
    #[serde(default)]
    grupo: Vec<GrupoToml>,
    #[serde(default)]
    esperado: Esperado,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BaseToml {
    arquivo: String,
    passo_s: Option<i64>,
    /// Semente do sorteio de `variacao_min`/`variacao_duracao` no `gerar-base`.
    #[serde(default)]
    semente: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CasaToml {
    p_max_w: f64,
    #[serde(default)]
    carga_fixa_w: f64,
    #[serde(default)]
    reserva: Vec<ReservaToml>,
    /// Quem decide: o MILP (padrao) ou uma regra de comparacao (`regras.rs`).
    #[serde(default)]
    decisao: crate::regras::Modo,
    #[serde(default)]
    usuario_atraso_s: i64,
    #[serde(default)]
    usuario_desiste_apos: Option<u32>,
    #[serde(default)]
    perda: Vec<PerdaToml>,
    /// Semente do sorteio das `[[casa.perda]]` com `prob < 1`.
    #[serde(default)]
    perda_semente: u64,
}

/// Perda de heartbeats (revisao do orientador 3.2): o anuncio que `envia` difunde no passo
/// nao chega a `recebe`, com probabilidade `prob`, dentro da `janela`. Campo ausente vale
/// para todas as tomadas, o dia todo, sempre. Nome de tomada ou de grupo.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PerdaToml {
    recebe: Option<String>,
    envia: Option<String>,
    #[serde(default = "um_f")]
    prob: f64,
    janela: Option<String>,
}

/// `[[casa.perda]]` validada.
#[derive(Debug, Clone)]
pub struct Perda {
    pub recebe: Option<String>,
    pub envia: Option<String>,
    pub prob: f64,
    /// `(inicio_min, fim_min)`.
    pub janela: Option<(u32, u32)>,
}

/// O morador simulado do `--realimentar` (revisao do orientador 4.6). O padrao e o de sempre:
/// aperta "usar agora" no mesmo passo e nunca desiste.
#[derive(Debug, Clone, Copy, Default)]
pub struct Usuario {
    /// Segundos com o aparelho querido e cortado antes de apertar "usar agora". Se o uso
    /// acabar antes, ele nao aperta: tudo vira uso negado.
    pub atraso_s: i64,
    /// Tomada que precisou de mais que isto de "usar agora" num dia vira essencial (critica)
    /// a partir do dia seguinte, como o morador que marca o aparelho como "sempre ligado".
    pub desiste_apos: Option<u32>,
}

/// Carga fora das tomadas, conhecida por horario (o chuveiro das 19:00): soma na
/// `carga_fixa` do passo dentro da janela, o que baixa o teto so nesse intervalo.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReservaToml {
    janela: String,
    w: f64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct LimitesToml {
    orcamento: Option<DuracaoToml>,
}

fn um_i() -> i64 {
    1
}
fn um_f() -> f64 {
    1.0
}
fn seiscentos() -> i64 {
    600
}
fn quinze() -> f64 {
    15.0
}

/// Grupo de tomadas (data-model §2.1). `perfil`, `w`, `uso_w`, `standby_w`, `janelas` e
/// `defasagem_min` servem so ao `gerar-base`; a simulacao nunca os le.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrupoToml {
    pub nome: String,
    #[serde(default = "um_i")]
    pub quantidade: i64,
    pub coluna: Option<String>,
    #[serde(default)]
    pub coluna_unica: bool,
    #[serde(default = "seiscentos")]
    pub limite_ociosidade_s: i64,
    #[serde(default = "quinze")]
    pub limiar_uso_w: f64,
    #[serde(default)]
    pub critica: bool,
    #[serde(default = "um_f")]
    pub peso: f64,
    #[serde(default)]
    pub horas_proibidas: Vec<i64>,
    pub perfil: Option<String>,
    pub w: Option<f64>,
    pub uso_w: Option<f64>,
    pub standby_w: Option<f64>,
    #[serde(default)]
    pub janelas: Vec<String>,
    #[serde(default)]
    pub defasagem_min: i64,
    /// Cada dia, o inicio de cada janela anda um sorteio uniforme em `[-v, +v]` minutos.
    #[serde(default)]
    pub variacao_min: i64,
    /// Cada dia, a duracao de cada janela e multiplicada por um sorteio em `[1-d, 1+d]`.
    #[serde(default)]
    pub variacao_duracao: f64,
    /// Janelas de sabado e domingo; ausente = as mesmas de `janelas`, vazio = sem uso.
    pub janelas_fds: Option<Vec<String>>,
}

/// Expectativas declaradas (data-model §2.2).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Esperado {
    pub cortes_min: Option<u64>,
    pub cortes_max: Option<u64>,
    #[serde(default)]
    pub sem_cortes: Vec<String>,
    pub economia_wh: Option<f64>,
    pub tolerancia: Option<f64>,
    pub economia_zero: Option<bool>,
    pub acima_do_teto_max: Option<u64>,
}

/// Uma tomada do caso, depois da expansao dos grupos.
#[derive(Debug, Clone)]
pub struct TomadaSim {
    pub nome: String,
    pub grupo: String,
    pub grupo_idx: usize,
    pub replica: usize,
    pub coluna: String,
    pub mac: [u8; 6],
    pub config: Tomada,
}

/// Caso validado.
pub struct Caso {
    pub bytes: Vec<u8>,
    pub nome: String,
    pub descricao: String,
    pub inicio: NaiveDateTime,
    pub duracao_s: i64,
    pub passo_s: i64,
    pub base_arquivo: PathBuf,
    pub base_passo_s: i64,
    pub base_semente: u64,
    pub p_max_w: f64,
    pub carga_fixa_w: f64,
    /// `(inicio_min, fim_min, w)` de cada `[[casa.reserva]]`.
    pub reservas: Vec<(u32, u32, f64)>,
    pub decisao: crate::regras::Modo,
    pub usuario: Usuario,
    pub perdas: Vec<Perda>,
    pub perda_semente: u64,
    pub orcamento_s: Option<i64>,
    pub grupos: Vec<GrupoToml>,
    pub tomadas: Vec<TomadaSim>,
    pub esperado: Esperado,
    pub avisos: Vec<String>,
}

impl Caso {
    pub fn passos_total(&self) -> u64 {
        (self.duracao_s / self.passo_s) as u64
    }

    /// Algum grupo declara perfil, entao a base pode ser gerada.
    pub fn tem_perfis(&self) -> bool {
        self.grupos.iter().any(|g| g.perfil.is_some())
    }
}

/// MAC sintetico da tomada de indice `i`: a ordenacao por MAC do `decidir` coincide com a
/// ordem do caso (research D2).
pub fn mac_sintetico(i: usize) -> [u8; 6] {
    [0, 0, 0, 0, (i >> 8) as u8, i as u8]
}

/// `"45s"`, `"90m"`, `"6h"`, `"30d"` ou um inteiro em segundos.
pub fn parse_duracao(campo: &str, texto: &str) -> Result<i64> {
    let s = texto.trim();
    let erro = || invalido(format!("{campo}: duração inválida \"{texto}\" (use \"45s\", \"90m\", \"6h\", \"30d\" ou segundos)"));
    if let Ok(n) = s.parse::<i64>() {
        return Ok(n);
    }
    let mult = match s.chars().last() {
        Some('s') => 1,
        Some('m') => 60,
        Some('h') => 3600,
        Some('d') => 86_400,
        _ => return Err(erro()),
    };
    let n: i64 = s[..s.len() - 1].trim().parse().map_err(|_| erro())?;
    n.checked_mul(mult).ok_or_else(erro)
}

fn duracao_toml(campo: &str, d: &DuracaoToml) -> Result<i64> {
    match d {
        DuracaoToml::Segundos(n) => Ok(*n),
        DuracaoToml::Texto(t) => parse_duracao(campo, t),
    }
}

fn parse_inicio(texto: &str) -> Result<NaiveDateTime> {
    for fmt in ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M", "%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M"] {
        if let Ok(dt) = NaiveDateTime::parse_from_str(texto.trim(), fmt) {
            return Ok(dt);
        }
    }
    Err(invalido(format!("inicio: data-hora inválida \"{texto}\" (use \"2026-01-05T00:00\")")))
}

/// `"19:00-23:00"` -> minutos `(1140, 1380)`. Pode atravessar a meia-noite (`"23:00-02:00"`).
pub fn parse_janela(texto: &str) -> Option<(u32, u32)> {
    let (a, b) = texto.trim().split_once('-')?;
    let min = |s: &str| -> Option<u32> {
        let (h, m) = s.trim().split_once(':')?;
        let h: u32 = h.parse().ok()?;
        let m: u32 = m.parse().ok()?;
        (h <= 24 && m < 60 && h * 60 + m <= 1440).then_some(h * 60 + m)
    };
    let (a, b) = (min(a)?, min(b)?);
    (a != b).then_some((a, b))
}

fn nome_valido(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn coluna_valida(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

pub fn carregar(caminho: &Path) -> Result<Caso> {
    let rotulo = caminho.display().to_string();
    let bytes = std::fs::read(caminho).map_err(|e| invalido(format!("caso {rotulo}: não consegui ler ({e})")))?;
    let texto = std::str::from_utf8(&bytes).map_err(|_| invalido(format!("caso {rotulo}: o arquivo não é UTF-8")))?;
    let bruto: CasoToml =
        toml::from_str(texto).map_err(|e| invalido(format!("caso {rotulo}: {}", e.to_string().trim_end())))?;
    let recusa = |msg: String| invalido(format!("caso {rotulo}: {msg}"));

    let nome = match bruto.nome {
        Some(n) => n,
        None => caminho.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
    };
    if !nome_valido(&nome) {
        return Err(recusa(format!("nome = \"{nome}\": use só letras minúsculas, dígitos e hífen")));
    }

    let inicio = parse_inicio(bruto.inicio.as_deref().unwrap_or(INICIO_PADRAO))?;
    let passo_s = bruto.passo_s.unwrap_or(10);
    if passo_s <= 0 {
        return Err(recusa(format!("passo_s = {passo_s}: tem que ser maior que zero")));
    }
    let duracao_s = duracao_toml("duracao", &bruto.duracao)?;
    if duracao_s <= 0 {
        return Err(recusa(format!("duracao = {duracao_s} s: tem que ser maior que zero")));
    }
    if duracao_s % passo_s != 0 {
        return Err(recusa(format!("duracao = {duracao_s} s não é múltipla de passo_s = {passo_s}")));
    }
    let base_passo_s = bruto.base.passo_s.unwrap_or(passo_s);
    if base_passo_s <= 0 {
        return Err(recusa(format!("base.passo_s = {base_passo_s}: tem que ser maior que zero")));
    }
    if passo_s > base_passo_s {
        return Err(recusa(format!(
            "passo_s = {passo_s} é maior que base.passo_s = {base_passo_s}: a leitura pegaria um valor por passo em vez da média do intervalo e a economia sairia errada sem aviso. Gere a base no passo desejado (FR-007)"
        )));
    }
    if bruto.base.arquivo.trim().is_empty() {
        return Err(recusa("base.arquivo vazio".to_string()));
    }
    let pasta_caso = caminho.parent().unwrap_or(Path::new("."));
    let base_arquivo = pasta_caso.join(&bruto.base.arquivo);

    let p_max_w = bruto.casa.p_max_w;
    if !p_max_w.is_finite() || p_max_w < 0.0 {
        return Err(recusa(format!("casa.p_max_w = {p_max_w}: tem que ser >= 0")));
    }
    let carga_fixa_w = bruto.casa.carga_fixa_w;
    if !carga_fixa_w.is_finite() || carga_fixa_w < 0.0 {
        return Err(recusa(format!("casa.carga_fixa_w = {carga_fixa_w}: tem que ser >= 0")));
    }
    let mut reservas = Vec::new();
    for r in &bruto.casa.reserva {
        let Some((a, b)) = parse_janela(&r.janela) else {
            return Err(recusa(format!("casa.reserva: janela \"{}\" inválida (use \"HH:MM-HH:MM\")", r.janela)));
        };
        if !r.w.is_finite() || r.w < 0.0 {
            return Err(recusa(format!("casa.reserva.w = {}: tem que ser >= 0", r.w)));
        }
        reservas.push((a, b, r.w));
    }
    if bruto.casa.usuario_atraso_s < 0 {
        return Err(recusa(format!("casa.usuario_atraso_s = {}: tem que ser >= 0", bruto.casa.usuario_atraso_s)));
    }
    let usuario = Usuario { atraso_s: bruto.casa.usuario_atraso_s, desiste_apos: bruto.casa.usuario_desiste_apos };
    let orcamento_s = match &bruto.limites.orcamento {
        Some(d) => {
            let s = duracao_toml("limites.orcamento", d)?;
            if s <= 0 {
                return Err(recusa(format!("limites.orcamento = {s} s: tem que ser maior que zero")));
            }
            Some(s)
        }
        None => None,
    };

    if bruto.grupo.is_empty() {
        return Err(recusa("nenhum [[grupo]]: a casa precisa de ao menos uma tomada".to_string()));
    }

    let mut avisos = Vec::new();
    let mut tomadas: Vec<TomadaSim> = Vec::new();
    let mut nomes_tomada: HashSet<String> = HashSet::new();
    let mut nomes_grupo: HashSet<String> = HashSet::new();
    for (gi, g) in bruto.grupo.iter().enumerate() {
        let ctx = format!("grupo \"{}\"", g.nome);
        if !nome_valido(&g.nome) {
            return Err(recusa(format!("{ctx}: nome inválido, use só letras minúsculas, dígitos e hífen")));
        }
        if !nomes_grupo.insert(g.nome.clone()) {
            return Err(recusa(format!("{ctx}: nome de grupo repetido")));
        }
        if g.quantidade < 1 {
            return Err(recusa(format!("{ctx}: quantidade = {} (mínimo 1)", g.quantidade)));
        }
        if !g.peso.is_finite() || g.peso < 1.0 {
            return Err(recusa(format!("{ctx}: peso = {} (mínimo 1)", g.peso)));
        }
        if g.limite_ociosidade_s <= 0 {
            return Err(recusa(format!("{ctx}: limite_ociosidade_s = {} (tem que ser > 0)", g.limite_ociosidade_s)));
        }
        if !g.limiar_uso_w.is_finite() || g.limiar_uso_w < 0.0 {
            return Err(recusa(format!("{ctx}: limiar_uso_w = {} (tem que ser >= 0)", g.limiar_uso_w)));
        }
        let mut schedule = [true; 24];
        for h in &g.horas_proibidas {
            if !(0..=23).contains(h) {
                return Err(recusa(format!("{ctx}: horas_proibidas contém {h}, fora de 0..23")));
            }
            schedule[*h as usize] = false;
        }
        match g.perfil.as_deref() {
            None | Some("constante") | Some("janelas") => {}
            Some(p) => return Err(recusa(format!("{ctx}: perfil = \"{p}\" desconhecido (use \"constante\" ou \"janelas\")"))),
        }
        for (campo, v) in [("w", g.w), ("uso_w", g.uso_w), ("standby_w", g.standby_w)] {
            if let Some(v) = v {
                if !v.is_finite() || v < 0.0 {
                    return Err(recusa(format!("{ctx}: {campo} = {v} (tem que ser >= 0)")));
                }
            }
        }
        for j in g.janelas.iter().chain(g.janelas_fds.iter().flatten()) {
            if parse_janela(j).is_none() {
                return Err(recusa(format!("{ctx}: janela \"{j}\" inválida (use \"HH:MM-HH:MM\")")));
            }
        }
        if g.variacao_min < 0 {
            return Err(recusa(format!("{ctx}: variacao_min = {} (tem que ser >= 0)", g.variacao_min)));
        }
        if !(0.0..1.0).contains(&g.variacao_duracao) {
            return Err(recusa(format!("{ctx}: variacao_duracao = {} (tem que estar em [0, 1))", g.variacao_duracao)));
        }
        if g.defasagem_min < 0 {
            return Err(recusa(format!("{ctx}: defasagem_min = {} (tem que ser >= 0)", g.defasagem_min)));
        }
        let aviso_limiar = match g.perfil.as_deref() {
            Some("constante") => g.w.filter(|w| *w > g.limiar_uso_w),
            Some("janelas") => g.standby_w.filter(|s| *s > g.limiar_uso_w),
            _ => None,
        };
        if let Some(v) = aviso_limiar {
            avisos.push(format!(
                "{ctx}: consumo fora de uso ({v} W) acima do limiar_uso_w ({} W). O aparelho parece \"em uso\" o tempo todo, o fantasma nunca é detectado e a rodada tende a sair verde com economia zero",
                g.limiar_uso_w
            ));
        }

        let coluna_base = g.coluna.clone().unwrap_or_else(|| g.nome.clone());
        if !coluna_valida(&coluna_base) {
            return Err(recusa(format!("{ctx}: coluna \"{coluna_base}\" inválida (sem vírgula, aspas ou espaço)")));
        }
        for r in 0..g.quantidade as usize {
            let nome_t = if r == 0 { g.nome.clone() } else { format!("{}-{}", g.nome, r + 1) };
            let coluna = if r == 0 || g.coluna_unica { coluna_base.clone() } else { format!("{coluna_base}-{}", r + 1) };
            if !nomes_tomada.insert(nome_t.clone()) {
                return Err(recusa(format!("tomada \"{nome_t}\" repetida depois da expansão dos grupos")));
            }
            let i = tomadas.len();
            if i >= MAX_TOMADAS {
                return Err(recusa(format!("mais de {MAX_TOMADAS} tomadas")));
            }
            let config = Tomada {
                is_critical: g.critica,
                w_i: g.peso as f32,
                p_max: p_max_w as f32,
                t_limite: g.limite_ociosidade_s,
                schedule,
                p_limiar_detectado_auto: Some(g.limiar_uso_w as f32),
                config_version: 0,
            };
            tomadas.push(TomadaSim {
                nome: nome_t,
                grupo: g.nome.clone(),
                grupo_idx: gi,
                replica: r,
                coluna,
                mac: mac_sintetico(i),
                config,
            });
        }
    }

    let mut perdas = Vec::new();
    for p in &bruto.casa.perda {
        for nome_p in p.recebe.iter().chain(&p.envia) {
            if !nomes_tomada.contains(nome_p) && !nomes_grupo.contains(nome_p) {
                return Err(recusa(format!("casa.perda cita \"{nome_p}\", que não é tomada nem grupo do caso")));
            }
        }
        if !(0.0..=1.0).contains(&p.prob) {
            return Err(recusa(format!("casa.perda.prob = {}: tem que estar em [0, 1]", p.prob)));
        }
        let janela = match &p.janela {
            Some(j) => Some(parse_janela(j).ok_or_else(|| recusa(format!("casa.perda: janela \"{j}\" inválida (use \"HH:MM-HH:MM\")")))?),
            None => None,
        };
        perdas.push(Perda { recebe: p.recebe.clone(), envia: p.envia.clone(), prob: p.prob, janela });
    }
    if !perdas.is_empty() && bruto.casa.decisao == crate::regras::Modo::Centralizado {
        return Err(recusa("casa.perda não combina com decisao = \"centralizado\", que não usa os anúncios".to_string()));
    }

    let esperado = bruto.esperado;
    for n in &esperado.sem_cortes {
        if !nomes_tomada.contains(n) && !nomes_grupo.contains(n) {
            return Err(recusa(format!("esperado.sem_cortes cita \"{n}\", que não é tomada nem grupo do caso")));
        }
    }
    if let Some(t) = esperado.tolerancia {
        if !t.is_finite() || t < 0.0 {
            return Err(recusa(format!("esperado.tolerancia = {t} (tem que ser >= 0)")));
        }
    }

    Ok(Caso {
        bytes,
        nome,
        descricao: bruto.descricao,
        inicio,
        duracao_s,
        passo_s,
        base_arquivo,
        base_passo_s,
        base_semente: bruto.base.semente,
        p_max_w,
        carga_fixa_w,
        reservas,
        decisao: bruto.casa.decisao,
        usuario,
        perdas,
        perda_semente: bruto.casa.perda_semente,
        orcamento_s,
        grupos: bruto.grupo,
        tomadas,
        esperado,
        avisos,
    })
}
