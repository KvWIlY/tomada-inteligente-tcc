//! O laco da rodada (data-model §4): N `Ciclo::decidir` por passo, cada tomada vendo as
//! vizinhas pelo anuncio do passo anterior (research D2).
//!
//! Nenhuma regra de decisao mora aqui (FR-002): o simulador le a base, monta a `Leitura`,
//! copia campos do `Decisao` para o anuncio, confere o que e observavel e acumula.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::Result;
use chrono::{Duration, NaiveDateTime, Timelike};
use sha2::{Digest, Sha256};

use crate::base::{Base, Leitor};
use crate::caso::{Caso, Perda, TomadaSim, Usuario};
use crate::decisao::ciclo::{Ciclo, Decisao};
use crate::firmware::hex;
use crate::formato::milhar;
use crate::histograma::Histograma;
use crate::invariantes::{self, Regra, Visao};
use crate::persistencia::config::Tomada;
use crate::progresso::{Progresso, PASSO_LENTO_S};
use crate::sensores::potencia;
use crate::solver::relogio;
use crate::solver::uso_agora::{self, Efeito, Janela, Origem};
use crate::solver::{Estado, TomadaInput};

/// `CICLO_S` do firmware. Passo maior que isto ganha o aviso da FR-007a.
pub const CICLO_S: i64 = 10;
/// `TTL_PEER_S` do `main.rs` do firmware: vizinha sem heartbeat ha mais que isto vira carga fixa.
pub const TTL_PEER_S: i64 = 3 * CICLO_S;
/// `esquecer_antigos(uptime, TTL_PEER_S * 10)` do firmware: depois disto a vizinha sai da tabela.
pub const ESQUECER_S: i64 = 10 * TTL_PEER_S;
/// Cortes de criticas guardados para a tabela do relatorio. Os seguintes so entram na
/// contagem: uma critica cortada em todo passo encheria a memoria e o relatorio.
pub const MAX_CORTES_CRITICAS_LISTADOS: usize = 1000;

/// A casa que o laco percorre: o caso expandido, ou uma versao redimensionada dele
/// (varredura).
pub struct Cenario {
    pub inicio: NaiveDateTime,
    pub passo_s: i64,
    pub passos_total: u64,
    pub p_max_w: f32,
    pub carga_fixa_w: f32,
    /// `[[casa.reserva]]` do caso: `(inicio_min, fim_min, w)`.
    pub reservas: Vec<(u32, u32, f64)>,
    pub usuario: Usuario,
    /// `[[casa.perda]]`: vazio = todo anuncio chega a todas, como sempre foi.
    pub perdas: Vec<Perda>,
    pub perda_semente: u64,
    pub tomadas: Vec<TomadaSim>,
    /// Coluna da base de cada tomada.
    pub colunas: Vec<usize>,
}

impl Cenario {
    pub fn de_caso(caso: &Caso, base: &Base, passos_total: u64) -> Cenario {
        Cenario {
            inicio: caso.inicio,
            passo_s: caso.passo_s,
            passos_total,
            p_max_w: caso.p_max_w as f32,
            carga_fixa_w: caso.carga_fixa_w as f32,
            reservas: caso.reservas.clone(),
            usuario: caso.usuario,
            perdas: caso.perdas.clone(),
            perda_semente: caso.perda_semente,
            tomadas: caso.tomadas.clone(),
            colunas: base.indices.clone(),
        }
    }

    /// O anuncio que `envia` difundiu no passo `k` se perdeu a caminho de `recebe`?
    fn perdido(&self, k: u64, recebe: usize, envia: usize) -> bool {
        let dt = self.instante(k);
        let m = dt.hour() * 60 + dt.minute();
        let casa = |nome: &Option<String>, i: usize| {
            nome.as_ref().is_none_or(|n| *n == self.tomadas[i].nome || *n == self.tomadas[i].grupo)
        };
        self.perdas.iter().enumerate().any(|(r, p)| {
            casa(&p.recebe, recebe)
                && casa(&p.envia, envia)
                && p.janela.is_none_or(|(a, b)| if a < b { m >= a && m < b } else { m >= a || m < b })
                && {
                    let x = [k, recebe as u64, envia as u64, r as u64]
                        .iter()
                        .fold(self.perda_semente, |h, &v| h.wrapping_mul(1_000_003).wrapping_add(v));
                    crate::gerar_base::sorteio(x) < p.prob
                }
        })
    }

    /// `carga_fixa` do passo: a fixa do caso mais as reservas cuja janela contem o instante.
    pub fn carga_fixa_em(&self, passo: u64) -> f32 {
        let dt = self.instante(passo);
        let m = dt.hour() * 60 + dt.minute();
        let extra: f64 = self
            .reservas
            .iter()
            .filter(|&&(a, b, _)| if a < b { m >= a && m < b } else { m >= a || m < b })
            .map(|r| r.2)
            .sum();
        self.carga_fixa_w + extra as f32
    }

    /// Teto disponivel como o solver o calcula: `max(p_max - carga_fixa, 0)`.
    pub fn teto_w(&self, passo: u64) -> f64 {
        (self.p_max_w as f64 - self.carga_fixa_em(passo) as f64).max(0.0)
    }

    pub fn instante(&self, passo: u64) -> NaiveDateTime {
        self.inicio + Duration::seconds(passo as i64 * self.passo_s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Desfecho {
    EmAndamento,
    Completou,
    ParouNoOrcamento,
    Falhou,
}

impl Desfecho {
    pub fn as_str(self) -> &'static str {
        match self {
            Desfecho::EmAndamento => "em_andamento",
            Desfecho::Completou => "completou",
            Desfecho::ParouNoOrcamento => "parou_no_orcamento",
            Desfecho::Falhou => "falhou",
        }
    }

    pub fn codigo(self) -> i32 {
        match self {
            Desfecho::Completou => 0,
            Desfecho::ParouNoOrcamento => 3,
            Desfecho::EmAndamento | Desfecho::Falhou => 1,
        }
    }
}

/// Evento de data-model §6: os do firmware com o texto intacto, mais os do simulador.
pub struct Evento {
    pub passo: u64,
    pub t_s: i64,
    pub instante: String,
    pub tomada: String,
    pub tipo: String,
    pub severidade: String,
    pub titulo: String,
    pub detalhes: String,
}

pub struct LinhaPasso {
    pub passo: u64,
    pub t_s: i64,
    pub decidir_total_us: u64,
    pub decidir_max_us: u32,
    pub tomadas_off: usize,
    pub consumo_modelo_w: f64,
    pub teto_w: f64,
    pub acima_do_teto: bool,
}

pub struct LinhaRastro {
    pub passo: u64,
    pub t_s: i64,
    pub tomada: usize,
    pub base_w: f32,
    pub u_i: bool,
    pub f_i: bool,
    pub estado: Estado,
    pub cstd: f32,
    pub con: f32,
    pub decidir_us: u32,
}

pub struct SaidaPasso {
    pub eventos: Vec<Evento>,
    pub linha: LinhaPasso,
    pub rastro: Vec<LinhaRastro>,
    /// A linha `FALHOU passo ...` da primeira violacao de invariante.
    pub violacao: Option<String>,
}

/// Acumuladores por tomada (data-model §5).
#[derive(Debug, Clone, Default)]
pub struct AcumTomada {
    pub energia_base_wh: f64,
    pub energia_com_sistema_wh: f64,
    pub cortes: u64,
    pub passos_off: u64,
    pub religamentos: u64,
    pub tempo_off_s: i64,
    pub tempo_std_s: i64,
    pub tempo_on_s: i64,
    pub penalidades_uso: u64,
    pub penalidades_age: u64,
    /// Vezes que o usuario simulado apertou "usar agora" (so com `--realimentar`).
    pub botoes: u64,
    /// Duracoes de `decidir` desta tomada, em memoria fixa.
    pub duracoes: Histograma,
}

impl AcumTomada {
    pub fn economia_wh(&self) -> f64 {
        self.energia_base_wh - self.energia_com_sistema_wh
    }
}

pub struct EstadoTomada {
    /// Estado do rele, calibracao e ociosidade: tudo dentro do `Ciclo` do firmware (FR-011).
    pub ciclo: Ciclo,
    /// O que a tomada anunciaria no heartbeat, para o passo seguinte.
    pub anuncio: Option<TomadaInput>,
    /// O `p_medida` do mesmo heartbeat: o que a tomada leu (0 W com o rele aberto).
    pub p_anunciado: f32,
    /// So com `[[casa.perda]]`: o ultimo heartbeat que ESTA tomada recebeu de cada uma, como a
    /// tabela de peers do `espnow.rs`, com o instante (t_s) em que chegou.
    pub vizinhas: Vec<Option<(TomadaInput, f32, i64)>>,
    pub estado_decidido: Option<Estado>,
    /// Janela de "usar agora" aberta pelo usuario simulado (so com `--realimentar`).
    pub janela: Option<Janela>,
    /// Desde quando o morador quer o aparelho e o acha cortado (`Usuario::atraso_s`).
    pub quer_desde: Option<i64>,
    /// Dia (desde o inicio) e "usar agora" apertados nele (`Usuario::desiste_apos`).
    pub dia: i64,
    pub botoes_hoje: u32,
    /// O morador marcou a tomada como essencial: a config do passo vai com `is_critical`.
    pub desistiu: bool,
    pub acc: AcumTomada,
}

pub struct CorteCritica {
    pub passo: u64,
    pub instante: String,
    pub tomada: String,
    pub teto_w: f64,
    pub consumo_modelo_w: f64,
}

pub struct PassoLento {
    pub passo: u64,
    pub instante: String,
    pub duracao_ms: u64,
}

/// Totais da casa.
#[derive(Debug, Clone, Copy, Default)]
pub struct TotaisCasa {
    pub energia_base_wh: f64,
    pub energia_com_sistema_wh: f64,
    pub cortes: u64,
    pub passos_off: u64,
}

impl TotaisCasa {
    pub fn economia_wh(&self) -> f64 {
        self.energia_base_wh - self.energia_com_sistema_wh
    }
}

pub struct Laco<'a> {
    cen: &'a Cenario,
    /// A linha corrente da base: a base nunca fica inteira em memoria.
    leitor: Leitor,
    pub estados: Vec<EstadoTomada>,
    /// Proximo passo a rodar, igual ao numero de passos concluidos.
    pub passo: u64,
    epoch0: i64,
    digest: Sha256,
    /// Digest parcial ao fim do ultimo passo. O `--determinismo` grava um por passo em disco
    /// (`registro::Digests`), nunca em memoria.
    pub ultimo_digest: [u8; 32],
    /// Duracoes de `decidir` da casa inteira, em memoria fixa.
    pub duracoes: Histograma,
    plantar: Option<Regra>,
    pub plantada: bool,
    /// Modo `realimenta`: OFF (e o rele aberto do boot) le 0 W, como na placa.
    pub realimentar: bool,
    rastro: bool,
    progresso: Option<Arc<Mutex<Progresso>>>,
    lista: Vec<([u8; 6], TomadaInput)>,
    peers: Vec<([u8; 6], TomadaInput)>,
    pub passos_acima_do_teto: u64,
    pub pior_excesso_w: f64,
    pub passo_mais_lento: Option<PassoLento>,
    /// Os primeiros `MAX_CORTES_CRITICAS_LISTADOS` cortes de criticas.
    pub cortes_criticas: Vec<CorteCritica>,
    pub cortes_criticas_total: u64,
    pub decidir_total_us: u64,
    totais: TotaisCasa,
}

// A copia do modo centralizado (`ptr::read`) so e segura enquanto o `Ciclo` nao tiver Drop.
const _: () = assert!(!std::mem::needs_drop::<Ciclo>());

/// Mapeamento de `Heartbeat::para_tomada_input` (`espnow.rs`): copia de campo, cada valor
/// vindo do `Decisao` ou da config do passo.
fn anuncio(d: &Decisao, config: &Tomada, a_it: bool) -> TomadaInput {
    TomadaInput {
        c_std: d.cstd as f64,
        c_on: d.con as f64,
        u_i: d.u_i,
        f_i: d.f_i,
        w_i: config.w_efetivo() as f64,
        a_it,
    }
}

fn instante_iso(dt: NaiveDateTime) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S").to_string()
}

impl<'a> Laco<'a> {
    pub fn novo(
        cen: &'a Cenario,
        base: &'a Base,
        plantar: Option<Regra>,
        rastro: bool,
        progresso: Option<Arc<Mutex<Progresso>>>,
    ) -> Result<Laco<'a>> {
        // O `inicio` e tratado como UTC so para virar epoch: sem fuso da maquina, o
        // determinismo nao depende do PC.
        let epoch0 = cen.inicio.and_utc().timestamp();
        // Como no boot da placa (`main.rs:217`): calibracao zero, rele aberto.
        let n = cen.tomadas.len();
        let estados = cen
            .tomadas
            .iter()
            .map(|t| EstadoTomada {
                ciclo: Ciclo::novo(&t.config, 0.0, 0.0, epoch0),
                anuncio: None,
                p_anunciado: 0.0,
                vizinhas: vec![None; n],
                estado_decidido: None,
                janela: None,
                quer_desde: None,
                dia: 0,
                botoes_hoje: 0,
                desistiu: false,
                acc: AcumTomada::default(),
            })
            .collect();
        Ok(Laco {
            cen,
            leitor: base.leitor()?,
            estados,
            passo: 0,
            epoch0,
            digest: Sha256::new(),
            ultimo_digest: [0; 32],
            duracoes: Histograma::default(),
            plantar,
            plantada: false,
            realimentar: false,
            rastro,
            progresso,
            lista: Vec::with_capacity(n),
            peers: Vec::with_capacity(n),
            passos_acima_do_teto: 0,
            pior_excesso_w: 0.0,
            passo_mais_lento: None,
            cortes_criticas: Vec::new(),
            cortes_criticas_total: 0,
            decidir_total_us: 0,
            totais: TotaisCasa::default(),
        })
    }

    pub fn totais(&self) -> TotaisCasa {
        self.totais
    }

    pub fn digest_hex(&self) -> String {
        format!("sha256:{}", hex(&self.digest.clone().finalize()))
    }

    /// Evento do proprio simulador, na casa, no passo em curso.
    pub fn evento_casa(&self, tipo: &str, severidade: &str, titulo: &str, detalhes: String) -> Evento {
        let t_s = self.passo as i64 * self.cen.passo_s;
        Evento {
            passo: self.passo,
            t_s,
            instante: instante_iso(self.cen.instante(self.passo)),
            tomada: "casa".to_string(),
            tipo: tipo.to_string(),
            severidade: severidade.to_string(),
            titulo: titulo.to_string(),
            detalhes,
        }
    }

    /// Roda um passo inteiro, na ordem da FR-010.
    pub fn passo(&mut self) -> Result<SaidaPasso> {
        let cen = self.cen;
        let n = cen.tomadas.len();
        let k = self.passo;

        // (1) Relogio do passo.
        let t_s = k as i64 * cen.passo_s;
        self.leitor.posicionar(t_s)?;
        let agora = self.epoch0 + t_s;
        let dt = cen.instante(k);
        let hora = dt.hour() as u8;
        let h = hora as usize;
        let instante = instante_iso(dt);
        let inicio_passo = Instant::now();
        if let Some(p) = &self.progresso {
            if let Ok(mut g) = p.lock() {
                g.inicio_passo = inicio_passo;
                g.em_passo = true;
                g.tomada_atual = 0;
            }
        }

        // (4) Anuncios do passo anterior, montados uma vez (vazio no passo 0).
        self.lista.clear();
        for (i, e) in self.estados.iter().enumerate() {
            if let Some(a) = e.anuncio {
                self.lista.push((cen.tomadas[i].mac, a));
            }
        }
        // Com perda, cada tomada so atualiza a sua tabela com o que chegou a ela (difundido no
        // passo anterior) e esquece quem esta calado ha mais de `ESQUECER_S`.
        if !cen.perdas.is_empty() && k > 0 {
            for i in 0..n {
                for j in 0..n {
                    let Some(a) = self.estados[j].anuncio else { continue };
                    if j != i && !cen.perdido(k - 1, i, j) {
                        let p = self.estados[j].p_anunciado;
                        self.estados[i].vizinhas[j] = Some((a, p, t_s - cen.passo_s));
                    }
                }
                for v in &mut self.estados[i].vizinhas {
                    if v.is_some_and(|(_, _, em)| t_s - em > ESQUECER_S) {
                        *v = None;
                    }
                }
            }
        }
        let mut ausentes: Vec<(usize, f32, usize)> = Vec::new();

        let mut decisoes: Vec<Decisao> = Vec::with_capacity(n);
        let mut leituras: Vec<f32> = Vec::with_capacity(n);
        let mut regras: Vec<bool> = Vec::with_capacity(n);
        let mut duracoes: Vec<u32> = Vec::with_capacity(n);
        let mut entradas: Vec<(potencia::Leitura, Efeito, Tomada)> = Vec::with_capacity(n);
        let mut desistencias: Vec<(usize, u32)> = Vec::new();
        let mut pendente: Option<u32> = None;
        let carga_fixa_w = cen.carga_fixa_em(k);
        for i in 0..n {
            let tom = &cen.tomadas[i];
            // (2) Leitura: a base, qualquer que seja o estado decidido antes (FR-006). Com
            // `--realimentar`, o rele aberto (OFF decidido, ou o boot) le 0 W; a energia
            // continua contada sobre a base, que e o que o aparelho puxaria energizado.
            let p = self.leitor.valor(cen.colunas[i]);
            let rele_aberto = !matches!(self.estados[i].estado_decidido, Some(Estado::Std | Estado::On));
            let p_lido = if self.realimentar && rele_aberto { 0.0 } else { p };
            let leitura = potencia::leitura(0.0, 0.0, p_lido);
            // Usuario simulado: quer usar (a base mostra uso), acha a tomada cortada e aperta
            // "usar agora", como `abrir_janela_uso` do `main.rs`. Sem isto, com realimentacao,
            // tomada cortada nunca mais veria uso. `agora` faz o papel do uptime.
            let mut efeito = Efeito::default();
            if self.realimentar {
                let u = cen.usuario;
                let e = &mut self.estados[i];
                let dia = t_s / 86_400;
                if dia != e.dia {
                    if u.desiste_apos.is_some_and(|n| e.botoes_hoje > n) && !e.desistiu {
                        e.desistiu = true;
                        desistencias.push((i, e.botoes_hoje));
                    }
                    e.dia = dia;
                    e.botoes_hoje = 0;
                }
                if uso_agora::expirou(e.janela, agora) {
                    e.janela = None;
                }
                let limiar = tom.config.p_limiar_detectado_auto.unwrap_or(15.0);
                if rele_aberto && p > limiar && e.janela.is_none() {
                    // Com `atraso_s = 0` aperta no mesmo passo, como sempre foi.
                    if agora - *e.quer_desde.get_or_insert(agora) >= u.atraso_s {
                        e.janela = Some(uso_agora::abrir(agora, Origem::Botao));
                        e.ciclo.marcar_uso_agora(agora);
                        e.acc.botoes += 1;
                        e.botoes_hoje += 1;
                        e.quer_desde = None;
                    }
                } else {
                    e.quer_desde = None;
                }
                efeito = uso_agora::efeito(e.janela, false, agora);
            }
            // (3) Regras horarias num clone da config, como `main.rs:359-361`.
            let rh = relogio::regras_horarias(true, tom.config.schedule[h]);
            let mut config_passo = tom.config.clone();
            config_passo.schedule[h] = rh.a_it;
            config_passo.is_critical |= self.estados[i].desistiu;
            leituras.push(p);
            regras.push(rh.a_it);
            entradas.push((leitura, efeito, config_passo));
        }

        if crate::regras::centralizado() {
            // Informacao perfeita: o anuncio DESTE passo, tirado de uma copia do `Ciclo`. O
            // `decidir` calibra e deriva U_i/F_i antes de resolver, sem olhar as vizinhas, entao
            // a copia anuncia exatamente o que a tomada vai usar como entrada propria abaixo.
            self.lista.clear();
            for i in 0..n {
                let tom = &cen.tomadas[i];
                let (leitura, efeito, config_passo) = &entradas[i];
                // SAFETY: `Ciclo` e dado puro (sem Drop, conferido pelo assert do topo); a copia
                // so e lida e descartada, o original nao muda.
                let mut copia = unsafe { std::ptr::read(&self.estados[i].ciclo) };
                let d = copia.decidir(config_passo, *leitura, &[], carga_fixa_w, tom.mac, None, *efeito, hora, agora);
                self.lista.push((tom.mac, anuncio(&d, config_passo, regras[i])));
            }
        }

        for i in 0..n {
            let tom = &cen.tomadas[i];
            let (leitura, efeito, config_passo) = &entradas[i];
            self.peers.clear();
            // `montar_coordenacao` do firmware: viva ate `TTL_PEER_S`, depois carga fixa com o
            // ultimo `p_medida`.
            let mut carga_ausentes = 0.0f32;
            if cen.perdas.is_empty() {
                self.peers.extend(self.lista.iter().filter(|(mac, _)| *mac != tom.mac).copied());
            } else {
                for (j, v) in self.estados[i].vizinhas.iter().enumerate() {
                    match v {
                        Some((a, _, em)) if t_s - em <= TTL_PEER_S => self.peers.push((cen.tomadas[j].mac, *a)),
                        Some((_, p, _)) => carga_ausentes += p,
                        None => {}
                    }
                }
                if k > 0 && self.peers.len() + 1 < n {
                    ausentes.push((i, carga_ausentes, self.peers.len()));
                }
            }

            if let Some(pg) = &self.progresso {
                if let Ok(mut g) = pg.lock() {
                    g.tomada_atual = i + 1;
                    if let Some(us) = pendente.take() {
                        g.registrar_chamada(us);
                    }
                }
            }
            let t0 = Instant::now();
            let d = self.estados[i].ciclo.decidir(
                config_passo,
                *leitura,
                &self.peers,
                carga_fixa_w + carga_ausentes,
                tom.mac,
                None,
                *efeito,
                hora,
                agora,
            );
            let us = t0.elapsed().as_micros().min(u32::MAX as u128) as u32;
            pendente = Some(us);
            duracoes.push(us);
            decisoes.push(d);
        }

        // (5) Invariantes observaveis, sobre uma copia que `--plantar` pode adulterar.
        let mut visoes: Vec<Visao> = decisoes
            .iter()
            .zip(&regras)
            .map(|(d, a_it)| Visao {
                estado: d.estado,
                u_i: d.u_i,
                f_i: d.f_i,
                a_it: *a_it,
                pen_uso: d.eventos.iter().any(|e| e.tipo == "penalidade_uso"),
                pen_age: d.eventos.iter().any(|e| e.tipo == "penalidade_age"),
            })
            .collect();
        if let Some(regra) = self.plantar {
            if !self.plantada && invariantes::plantar(regra, &mut visoes) {
                self.plantada = true;
            }
        }
        let violacao = invariantes::conferir(&visoes, n).map(|(idx, desc)| {
            let tomada = idx.map(|i| cen.tomadas[i].nome.as_str()).unwrap_or("casa");
            format!("FALHOU passo {} ({}) tomada {}: {}", milhar(k), dt.format("%Y-%m-%d %H:%M:%S"), tomada, desc)
        });

        // (8) Teto somado como metrica (FR-019a), com o `cstd`/`con` que cada tomada anunciaria.
        let consumo_modelo_w: f64 = decisoes
            .iter()
            .map(|d| match d.estado {
                Estado::Off => 0.0,
                Estado::Std => d.cstd as f64,
                Estado::On => d.con as f64,
            })
            .sum();
        let teto_w = cen.teto_w(k);
        let acima_do_teto = consumo_modelo_w > teto_w + 1e-6;
        if acima_do_teto {
            self.passos_acima_do_teto += 1;
            self.pior_excesso_w = self.pior_excesso_w.max(consumo_modelo_w - teto_w);
        }

        // (6) Energia, acumuladores, eventos, digest, rastro. (7) Novo anuncio.
        self.digest.update(k.to_le_bytes());
        let mut eventos = Vec::new();
        let mut rastro = Vec::new();
        let mut tomadas_off = 0usize;
        let wh_por_w = cen.passo_s as f64 / 3600.0;
        for (i, d) in decisoes.iter().enumerate() {
            let tom = &cen.tomadas[i];
            let e = &mut self.estados[i];
            let a = &mut e.acc;
            // A decisao do passo k vale para o intervalo [t_k, t_k+1) (research D3).
            let wh = leituras[i] as f64 * wh_por_w;
            a.energia_base_wh += wh;
            self.totais.energia_base_wh += wh;
            if d.estado != Estado::Off {
                a.energia_com_sistema_wh += wh;
                self.totais.energia_com_sistema_wh += wh;
            }
            match d.estado {
                Estado::Off => {
                    a.tempo_off_s += cen.passo_s;
                    a.passos_off += 1;
                    self.totais.passos_off += 1;
                    tomadas_off += 1;
                }
                Estado::Std => a.tempo_std_s += cen.passo_s,
                Estado::On => a.tempo_on_s += cen.passo_s,
            }
            // O passo 0 parte do OFF de boot, nao de uma decisao: nao conta transicao.
            if let Some(antes) = e.estado_decidido {
                if antes != Estado::Off && d.estado == Estado::Off {
                    a.cortes += 1;
                    self.totais.cortes += 1;
                    if tom.config.is_critical {
                        self.cortes_criticas_total += 1;
                        if self.cortes_criticas.len() < MAX_CORTES_CRITICAS_LISTADOS {
                            self.cortes_criticas.push(CorteCritica {
                                passo: k,
                                instante: instante.clone(),
                                tomada: tom.nome.clone(),
                                teto_w,
                                consumo_modelo_w,
                            });
                        }
                    }
                }
                if antes == Estado::Off && d.estado != Estado::Off {
                    a.religamentos += 1;
                }
            }
            e.estado_decidido = Some(d.estado);
            a.duracoes.registrar(duracoes[i]);
            self.duracoes.registrar(duracoes[i]);

            self.digest.update([d.estado as u8, d.u_i as u8, d.f_i as u8]);
            for ev in &d.eventos {
                match ev.tipo {
                    "penalidade_uso" => a.penalidades_uso += 1,
                    "penalidade_age" => a.penalidades_age += 1,
                    _ => {}
                }
                self.digest.update(ev.tipo.as_bytes());
                self.digest.update([0u8]);
                self.digest.update(ev.titulo.as_bytes());
                self.digest.update([0u8]);
                self.digest.update(ev.detalhes.as_bytes());
                self.digest.update([0u8]);
                eventos.push(Evento {
                    passo: k,
                    t_s,
                    instante: instante.clone(),
                    tomada: tom.nome.clone(),
                    tipo: ev.tipo.to_string(),
                    severidade: ev.severidade.to_string(),
                    titulo: ev.titulo.clone(),
                    detalhes: ev.detalhes.clone(),
                });
            }

            if self.rastro {
                rastro.push(LinhaRastro {
                    passo: k,
                    t_s,
                    tomada: i,
                    base_w: leituras[i],
                    u_i: d.u_i,
                    f_i: d.f_i,
                    estado: d.estado,
                    cstd: d.cstd,
                    con: d.con,
                    decidir_us: duracoes[i],
                });
            }

            e.anuncio = Some(anuncio(d, &entradas[i].2, regras[i]));
            e.p_anunciado = entradas[i].0.p;
        }
        // Fora do digest: so existe com `[[casa.perda]]`.
        for (i, l_w, vivas) in ausentes {
            eventos.push(Evento {
                passo: k,
                t_s,
                instante: instante.clone(),
                tomada: cen.tomadas[i].nome.clone(),
                tipo: "vizinha_ausente".to_string(),
                severidade: "info".to_string(),
                titulo: "vizinha fora do TTL ou esquecida".to_string(),
                detalhes: format!("carga_ausentes_w={l_w:.1} vivas={vivas}"),
            });
        }
        for (i, botoes) in desistencias {
            eventos.push(Evento {
                passo: k,
                t_s,
                instante: instante.clone(),
                tomada: cen.tomadas[i].nome.clone(),
                tipo: "desistencia".to_string(),
                severidade: "info".to_string(),
                titulo: "morador marcou a tomada como essencial".to_string(),
                detalhes: format!("{botoes} usar agora no dia anterior"),
            });
        }
        self.ultimo_digest = self.digest.clone().finalize().into();

        let decidir_total_us: u64 = duracoes.iter().map(|&u| u as u64).sum();
        let decidir_max_us = duracoes.iter().copied().max().unwrap_or(0);
        self.decidir_total_us += decidir_total_us;

        let parede = inicio_passo.elapsed();
        let parede_ms = parede.as_millis() as u64;
        if self.passo_mais_lento.as_ref().is_none_or(|p| parede_ms > p.duracao_ms) {
            self.passo_mais_lento = Some(PassoLento { passo: k, instante: instante.clone(), duracao_ms: parede_ms });
        }
        if parede.as_secs_f64() > PASSO_LENTO_S as f64 {
            eventos.push(Evento {
                passo: k,
                t_s,
                instante: instante.clone(),
                tomada: "casa".to_string(),
                tipo: "passo_lento".to_string(),
                severidade: "warn".to_string(),
                titulo: format!("Passo acima de {PASSO_LENTO_S} s"),
                detalhes: format!("duracao_ms={parede_ms} tomadas={n}"),
            });
        }

        self.passo += 1;
        if let Some(pg) = &self.progresso {
            if let Ok(mut g) = pg.lock() {
                if let Some(us) = pendente.take() {
                    g.registrar_chamada(us);
                }
                g.passo = self.passo;
                g.em_passo = false;
                g.energia_base_wh = self.totais.energia_base_wh;
                g.energia_com_sistema_wh = self.totais.energia_com_sistema_wh;
                g.cortes = self.totais.cortes;
            }
        }

        Ok(SaidaPasso {
            eventos,
            linha: LinhaPasso {
                passo: k,
                t_s,
                decidir_total_us,
                decidir_max_us,
                tomadas_off,
                consumo_modelo_w,
                teto_w,
                acima_do_teto,
            },
            rastro,
            violacao,
        })
    }
}

/// Uma expectativa declarada, avaliada no fim da rodada.
pub struct Expectativa {
    pub descricao: String,
    pub passou: bool,
    pub obtido: String,
}

/// Expectativas de data-model §2.2 (FR-021).
pub fn avaliar_expectativas(caso: &Caso, laco: &Laco) -> Vec<Expectativa> {
    let esp = &caso.esperado;
    let tot = laco.totais();
    let mut out = Vec::new();
    if let Some(m) = esp.cortes_min {
        out.push(Expectativa {
            descricao: format!("cortes_min = {m}"),
            passou: tot.cortes >= m,
            obtido: format!("cortes = {}", tot.cortes),
        });
    }
    if let Some(m) = esp.cortes_max {
        out.push(Expectativa {
            descricao: format!("cortes_max = {m}"),
            passou: tot.cortes <= m,
            obtido: format!("cortes = {}", tot.cortes),
        });
    }
    for nome in &esp.sem_cortes {
        let (mut cortes, mut passos_off) = (0u64, 0u64);
        for (t, e) in caso.tomadas.iter().zip(&laco.estados) {
            if t.nome == *nome || t.grupo == *nome {
                cortes += e.acc.cortes;
                passos_off += e.acc.passos_off;
            }
        }
        out.push(Expectativa {
            descricao: format!("sem_cortes \"{nome}\""),
            passou: cortes == 0 && passos_off == 0,
            obtido: format!("cortes = {cortes}, passos_off = {passos_off}"),
        });
    }
    let economia = tot.economia_wh();
    if let Some(alvo) = esp.economia_wh {
        let tol = esp.tolerancia.unwrap_or(0.01);
        let passou = if alvo == 0.0 { economia.abs() <= 1e-6 } else { ((economia - alvo) / alvo).abs() <= tol };
        out.push(Expectativa {
            descricao: format!("economia_wh = {alvo} (tolerância {tol})"),
            passou,
            obtido: format!("economia_wh = {economia:.3}"),
        });
    }
    if esp.economia_zero == Some(true) {
        out.push(Expectativa {
            descricao: "economia_zero = true".to_string(),
            passou: economia == 0.0,
            obtido: format!("economia_wh = {economia:.3}"),
        });
    }
    if let Some(m) = esp.acima_do_teto_max {
        out.push(Expectativa {
            descricao: format!("acima_do_teto_max = {m}"),
            passou: laco.passos_acima_do_teto <= m,
            obtido: format!("passos_acima_do_teto = {}", laco.passos_acima_do_teto),
        });
    }
    out
}

/// Mediana, p90 e maximo das duracoes.
pub fn estatisticas(duracoes: &Histograma) -> (u32, u32, u32) {
    let q = duracoes.percentis(&[0.5, 0.9, 1.0]);
    (q[0], q[1], q[2])
}
