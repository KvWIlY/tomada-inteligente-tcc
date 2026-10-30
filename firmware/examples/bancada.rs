//! Bancada do item 17 do parecer (2026-10-22): mede, no ESP32, o `Ciclo::decidir` do firmware
//! (o mesmo que roda a cada 10 s) com 1 a 20 tomadas sinteticas, nos tres casos de `Caso`.
//! Um caso para de crescer quando uma decisao passa de `LIMITE_US`.
//! Sem sensor, sem Wi-Fi: so a decisao. E um example do proprio pacote para herdar o perfil
//! do firmware (opt-level 0 no y-firmware) sem mudar nenhum fonte dele.
//!
//! cd firmware && cargo build --example bancada
//! espflash flash --port COMx --partition-table partitions.csv ../target/xtensa-esp32-espidf/debug/examples/bancada
//! O resumo se repete na serial (115200) a cada 10 s.

#[allow(dead_code, unused_imports)]
#[path = "../src/solver/mod.rs"]
mod solver;

#[allow(dead_code, unused_imports)]
mod sensores {
    #[path = "../../src/sensores/potencia.rs"]
    pub mod potencia;
}

#[allow(dead_code, unused_imports)]
mod persistencia {
    #[path = "../../src/persistencia/config.rs"]
    pub mod config;
    /// O `config.rs` chama `littlefs::base()` em `load`/`save`, que a bancada nunca chama.
    pub mod littlefs {
        pub fn base() -> &'static str {
            "."
        }
    }
}

#[allow(dead_code, unused_imports)]
mod decisao {
    #[path = "../../src/decisao/ciclo.rs"]
    pub mod ciclo;
}

use std::time::Duration;

use esp_idf_svc::sys;

use decisao::ciclo::Ciclo;
use persistencia::config::Tomada;
use sensores::potencia;
use solver::uso_agora::Efeito;
use solver::TomadaInput;

const TAMANHOS: [usize; 7] = [1, 2, 4, 8, 12, 16, 20];
const PILHA: usize = 64 * 1024;
/// Um caso para de crescer em `n` quando uma decisao passa disto.
const LIMITE_US: i64 = 60_000_000;
/// Decisoes por configuracao (revisao do orientador 4.5: dispersao, p99 e maximo), ate
/// `ORCAMENTO_US` somados; no minimo uma.
const REPETICOES: usize = 200;
const ORCAMENTO_US: i64 = 120_000_000;
/// Potencias de uso das vizinhas (W), as dos cenarios do TCC.
const USO_W: [f64; 10] = [1500.0, 1400.0, 1000.0, 600.0, 750.0, 1146.0, 200.0, 150.0, 80.0, 46.0];

#[derive(Clone, Copy, PartialEq)]
enum Caso {
    /// Metade em uso, teto no dobro da soma: nunca aperta (cenarios 1-4).
    Folgado,
    /// A propria e 3 vizinhas em uso, o resto ocioso; o teto obriga a cortar ~700 W (cenario 6).
    Teto4,
    /// Todas em uso e o teto na metade da soma: o pior caso da busca.
    TetoTodas,
}

impl Caso {
    fn nome(self) -> &'static str {
        match self {
            Caso::Folgado => "folgado",
            Caso::Teto4 => "teto-4",
            Caso::TetoTodas => "teto-todas",
        }
    }
}

fn agora_us() -> i64 {
    unsafe { sys::esp_timer_get_time() }
}

/// p25, mediana, p75, p99 e maximo em us (quantil pelo vizinho mais proximo).
fn quantis(mut v: Vec<i64>) -> [i64; 5] {
    v.sort_unstable();
    let q = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize];
    [q(0.25), q(0.5), q(0.75), q(0.99), v[v.len() - 1]]
}

/// Roda as decisoes da propria tomada com `n - 1` vizinhas: a linha do resumo e o maior tempo.
fn medir(n: usize, caso: Caso) -> (String, i64) {
    let vizinhas: Vec<([u8; 6], TomadaInput)> = (1..n)
        .map(|j| {
            let em_uso = match caso {
                Caso::Folgado => j % 2 == 0,
                Caso::Teto4 => j <= 3,
                Caso::TetoTodas => true,
            };
            let t = TomadaInput {
                c_std: 0.5 + (j % 5) as f64,
                c_on: USO_W[j % USO_W.len()],
                u_i: em_uso,
                f_i: !em_uso && j % 4 == 1,
                w_i: if j % 2 == 0 { 10.0 } else { 3.0 },
                a_it: true,
            };
            ([0, 0, 0, 0, 0, j as u8], t)
        })
        .collect();
    let propria_w = if caso == Caso::Folgado { 2.0 } else { 1000.0 };
    let soma: f64 = propria_w + vizinhas.iter().filter(|(_, t)| t.u_i).map(|(_, t)| t.c_on).sum::<f64>();
    let p_max = match caso {
        Caso::Folgado => 2.0 * soma,
        Caso::Teto4 => soma - 700.0,
        Caso::TetoTodas => soma / 2.0,
    };
    let config = Tomada { w_i: 10.0, p_max: p_max as f32, ..Tomada::default() };
    let mut ciclo = Ciclo::novo(&config, 0.0, 0.0, 0);
    let mut tempos = Vec::with_capacity(REPETICOES);
    let mut estado = "";
    // FNV-1a dos estados decididos: variantes de compilacao tem que decidir igual.
    let mut digest: u32 = 0x811c9dc5;
    for k in 0..REPETICOES as i64 {
        let leitura = potencia::leitura(0.0, 0.0, propria_w as f32);
        let t0 = agora_us();
        let d = ciclo.decidir(&config, leitura, &vizinhas, 0.0, [0; 6], None, Efeito::default(), 12, k * 10);
        let dt = agora_us() - t0;
        tempos.push(dt);
        estado = d.estado.as_str();
        digest = (digest ^ d.estado as u32).wrapping_mul(0x01000193);
        // Fora da medicao: sem isto a IDLE0 nao roda e o task watchdog reclama.
        std::thread::sleep(Duration::from_millis(1));
        if dt > LIMITE_US || tempos.iter().sum::<i64>() > ORCAMENTO_US {
            break;
        }
    }
    let decisoes = tempos.len();
    let [p25, med, p75, p99, max] = quantis(tempos);
    let pilha_livre = unsafe { sys::uxTaskGetStackHighWaterMark(std::ptr::null_mut()) } as usize;
    let heap_min = unsafe { sys::esp_get_minimum_free_heap_size() };
    let linha = format!(
        "{:<10} n={:>2} | {:>3} decisoes | mediana {:>8} us, IQR {:>8}-{:<8} us, p99 {:>8} us, max {:>8} us | estado {:<3} {:08x} | pilha usada ate aqui {:>5} B | heap livre minimo {:>6} B",
        caso.nome(),
        n,
        decisoes,
        med,
        p25,
        p75,
        p99,
        max,
        estado,
        digest,
        PILHA - pilha_livre,
        heap_min
    );
    (linha, max)
}

fn main() {
    sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let heap_antes = unsafe { sys::esp_get_free_heap_size() };
    let mac = {
        let mut m = [0u8; 6];
        unsafe { sys::esp_efuse_mac_get_default(m.as_mut_ptr()) };
        m.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":")
    };
    let cabecalho = format!(
        "=== bancada item 17 | placa {mac} | CPU {} MHz | heap livre antes {heap_antes} B | pilha da thread {PILHA} B",
        unsafe { sys::esp_rom_get_cpu_ticks_per_us() }
    );
    println!("{cabecalho}");
    // Thread com pilha conhecida: a marca d'agua dela e so a da decisao.
    let linhas = std::thread::Builder::new()
        .stack_size(PILHA)
        .spawn(|| {
            let mut linhas = Vec::new();
            for caso in [Caso::Folgado, Caso::Teto4, Caso::TetoTodas] {
                for n in TAMANHOS {
                    let (l, max) = medir(n, caso);
                    println!("{l}");
                    linhas.push(l);
                    if max > LIMITE_US {
                        let l = format!("{:<10} parei: decidir passou de {} s", caso.nome(), LIMITE_US / 1_000_000);
                        println!("{l}");
                        linhas.push(l);
                        break;
                    }
                }
            }
            linhas
        })
        .expect("thread da bancada")
        .join()
        .expect("bancada terminou com panico");

    loop {
        println!("{cabecalho}");
        for l in &linhas {
            println!("{l}");
        }
        println!("=== fim");
        std::thread::sleep(Duration::from_secs(10));
    }
}
