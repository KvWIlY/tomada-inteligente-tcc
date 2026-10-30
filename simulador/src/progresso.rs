//! Thread de progresso (research D5): fala a cada `--progresso` segundos mesmo quando o
//! laco esta parado dentro de um solve longo. O laco publica num `Mutex`, e o lock nunca e
//! segurado durante um `decidir`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::estimativa;
use crate::formato::{hms, milhar, virgula};
use crate::histograma::Histograma;

/// Janela de medicao da estimativa (research D6).
pub const JANELA_ESTIMATIVA_S: u64 = 20;
/// Acima disto, o passo em curso ganha a segunda linha no progresso (e o evento `passo_lento`).
pub const PASSO_LENTO_S: u64 = 10;

pub struct Progresso {
    /// Passos concluidos.
    pub passo: u64,
    pub passos_total: u64,
    pub inicio_rodada: Instant,
    pub inicio_passo: Instant,
    pub em_passo: bool,
    /// Tomada em que o passo esta (1..N).
    pub tomada_atual: usize,
    pub n_tomadas: usize,
    pub energia_base_wh: f64,
    pub energia_com_sistema_wh: f64,
    pub cortes: u64,
    /// Chamadas de `decidir` concluidas.
    pub chamadas: u64,
    /// Duracao de cada `decidir` (us), enquanto a estimativa coleta.
    pub amostras: Histograma,
    pub coletando: bool,
    pub projecao_impressa: bool,
}

impl Progresso {
    fn novo(passos_total: u64, n_tomadas: usize, coletando: bool) -> Self {
        let agora = Instant::now();
        Progresso {
            passo: 0,
            passos_total,
            inicio_rodada: agora,
            inicio_passo: agora,
            em_passo: false,
            tomada_atual: 0,
            n_tomadas,
            energia_base_wh: 0.0,
            energia_com_sistema_wh: 0.0,
            cortes: 0,
            chamadas: 0,
            amostras: Histograma::default(),
            coletando,
            projecao_impressa: false,
        }
    }

    pub fn registrar_chamada(&mut self, us: u32) {
        self.chamadas += 1;
        if self.coletando {
            self.amostras.registrar(us);
        }
    }

    fn linhas(&self) -> String {
        let decorrido = self.inicio_rodada.elapsed().as_secs_f64();
        let pct = if self.passos_total == 0 { 100 } else { self.passo * 100 / self.passos_total };
        let fim = if self.passo == 0 {
            "?".to_string()
        } else {
            hms(decorrido / self.passo as f64 * (self.passos_total - self.passo) as f64)
        };
        let mut s = format!(
            "[{pct:>3}%] passo {}/{}  decorrido {}  fim em ~{}  base={} kWh  com={} kWh  cortes={}",
            milhar(self.passo),
            milhar(self.passos_total),
            hms(decorrido),
            fim,
            virgula(self.energia_base_wh / 1000.0, 1),
            virgula(self.energia_com_sistema_wh / 1000.0, 1),
            milhar(self.cortes)
        );
        let no_passo = self.inicio_passo.elapsed().as_secs();
        if self.em_passo && no_passo > PASSO_LENTO_S {
            s.push_str(&format!(
                "\n       passo atual resolvendo ha {} s (tomada {}/{})",
                no_passo, self.tomada_atual, self.n_tomadas
            ));
        }
        s
    }
}

/// O que a thread faz aos 20 s.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Projecao {
    /// Nada (segunda passada do determinismo, varredura).
    Nenhuma,
    /// Imprime a projecao no stderr e a rodada segue de onde estava.
    Rodar,
    /// Imprime no stdout e encerra o processo com codigo 0 (`estimar`).
    Estimar,
}

pub struct Monitor {
    estado: Arc<Mutex<Progresso>>,
    parar: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Monitor {
    pub fn iniciar(intervalo_s: u64, passos_total: u64, n_tomadas: usize, projecao: Projecao) -> Monitor {
        let estado = Arc::new(Mutex::new(Progresso::novo(passos_total, n_tomadas, projecao != Projecao::Nenhuma)));
        let parar = Arc::new(AtomicBool::new(false));
        let (e, p) = (Arc::clone(&estado), Arc::clone(&parar));
        let intervalo = Duration::from_secs(intervalo_s.max(1));
        let handle = std::thread::spawn(move || {
            let mut ultimo = Instant::now();
            while !p.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(100));
                let mut g = match e.lock() {
                    Ok(g) => g,
                    Err(_) => return,
                };
                if ultimo.elapsed() >= intervalo {
                    eprintln!("{}", g.linhas());
                    ultimo = Instant::now();
                }
                if projecao != Projecao::Nenhuma
                    && !g.projecao_impressa
                    && g.inicio_rodada.elapsed() >= Duration::from_secs(JANELA_ESTIMATIVA_S)
                {
                    let texto = estimativa::texto_projecao(&g);
                    g.projecao_impressa = true;
                    g.coletando = false;
                    match projecao {
                        Projecao::Estimar => {
                            println!("{texto}");
                            std::process::exit(0);
                        }
                        _ => eprintln!("{texto}"),
                    }
                }
            }
        });
        Monitor { estado, parar, handle: Some(handle) }
    }

    pub fn estado(&self) -> Arc<Mutex<Progresso>> {
        Arc::clone(&self.estado)
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        self.parar.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}
