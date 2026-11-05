//! Mede, no ESP32, o `Ciclo::decidir` do firmware (o mesmo que roda a cada 10 s) com 2 a 30
//! tomadas sinteticas (revisao do orientador de 31/10: 1.000 decisoes, mediana e p99), em
//! instancias sorteadas a cada decisao, nos cinco casos de `Caso`, em tres fases: sem radio com
//! duas sementes e com radio na primeira semente, que deve repetir os digests da fase sem radio.
//! Um caso para de crescer quando uma decisao passa de `LIMITE_US`.
//! Sem sensor: so a decisao, em CPU0 e prioridade 1, como a tarefa principal do firmware. E um
//! example do proprio pacote para herdar o perfil do firmware (opt-level 0 no y-firmware) sem
//! mudar nenhum fonte dele. O radio fica sem roteador e sem receber, nao em modem-sleep, enviando
//! 10 quadros de 44 B/s, 100 vezes o heartbeat real: carga de pior caso, nao a operacao normal.
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

use esp_idf_svc::espnow::EspNow;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::cpu::Core;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::hal::task::thread::ThreadSpawnConfiguration;
use esp_idf_svc::sys;
use esp_idf_svc::wifi::{ClientConfiguration, Configuration, EspWifi};

use decisao::ciclo::Ciclo;
use persistencia::config::Tomada;
use sensores::potencia;
use solver::uso_agora::Efeito;
use solver::TomadaInput;

const TAMANHOS: [usize; 11] = [2, 4, 6, 8, 10, 13, 16, 17, 18, 20, 30];
const CASOS: [Caso; 5] = [Caso::Folgado, Caso::Teto4, Caso::TetoTodas, Caso::Pior, Caso::Mochila];
/// (com radio, rodada). A rodada desloca a semente; a 0 repete as instancias de 04/11/2026.
const FASES: [(bool, u64); 3] = [(false, 0), (false, 1), (true, 0)];
const BROADCAST: [u8; 6] = [0xff; 6];
const PILHA: usize = 64 * 1024;
/// Um caso para de crescer em `n` quando uma decisao passa disto.
const LIMITE_US: i64 = 60_000_000;
/// Decisoes por configuracao (revisao do orientador 4.5: dispersao, p99 e maximo), ate
/// `ORCAMENTO_US` somados; no minimo uma.
const REPETICOES: usize = 1000;
const ORCAMENTO_US: i64 = 1_200_000_000;
/// Potencias de uso das vizinhas (W), as dos cenarios do TCC.
const USO_W: [f64; 10] = [1500.0, 1400.0, 1000.0, 600.0, 750.0, 1146.0, 200.0, 150.0, 80.0, 46.0];

#[derive(Clone, Copy, PartialEq)]
enum Caso {
    /// Metade em uso, teto no dobro da soma: nunca aperta (cenarios 1-4).
    Folgado,
    /// A propria e 3 vizinhas em uso, o resto ocioso; o teto obriga a cortar ~700 W (cenario 6).
    Teto4,
    /// Todas em uso e o teto na metade da soma.
    TetoTodas,
    /// Como `TetoTodas`, com potencias quase iguais: com 20 tomadas, a busca chega ao limite de nos.
    Pior,
    /// Todas em uso, potencias e pesos (`W_i` em escala de watts) sorteados sem relacao entre si.
    Mochila,
}

impl Caso {
    fn nome(self) -> &'static str {
        match self {
            Caso::Folgado => "folgado",
            Caso::Teto4 => "teto-4",
            Caso::TetoTodas => "teto-todas",
            Caso::Pior => "pior",
            Caso::Mochila => "mochila",
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

/// Sorteio reproduzivel (splitmix64) em [0, 1).
fn sorteio(s: &mut u64) -> f64 {
    *s = s.wrapping_add(0x9e3779b97f4a7c15);
    let mut z = *s;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
}

/// Uma instancia sorteada: as vizinhas e o teto. Mesma semente, mesma sequencia em toda placa.
fn instancia(n: usize, caso: Caso, propria_w: f64, s: &mut u64) -> (Vec<([u8; 6], TomadaInput)>, f64) {
    let giro = (sorteio(s) * n as f64) as usize;
    let vizinhas: Vec<([u8; 6], TomadaInput)> = (1..n)
        .map(|j| {
            let em_uso = match caso {
                Caso::Folgado => sorteio(s) < 0.5,
                Caso::Teto4 => (j + giro) % n <= 3,
                Caso::TetoTodas | Caso::Pior | Caso::Mochila => true,
            };
            let c_on = if caso == Caso::Pior {
                // Potencias quase iguais: a poda quase nao corta, e a busca vai ate o limite de nos.
                1000.0 + sorteio(s)
            } else if caso == Caso::Mochila {
                100.0 + 1400.0 * sorteio(s)
            } else {
                USO_W[j % USO_W.len()] * (0.8 + 0.4 * sorteio(s))
            };
            let t = TomadaInput {
                c_std: 0.5 + (j % 5) as f64,
                c_on,
                u_i: em_uso,
                f_i: !em_uso && sorteio(s) < 0.25,
                w_i: if caso == Caso::Mochila {
                    100.0 + 2900.0 * sorteio(s)
                } else if j % 2 == 0 {
                    10.0
                } else {
                    3.0
                },
                a_it: true,
            };
            ([0, 0, 0, 0, 0, j as u8], t)
        })
        .collect();
    let soma: f64 = propria_w + vizinhas.iter().filter(|(_, t)| t.u_i).map(|(_, t)| t.c_on).sum::<f64>();
    let p_max = match caso {
        Caso::Folgado => 2.0 * soma,
        Caso::Teto4 => soma - 700.0,
        Caso::TetoTodas | Caso::Pior | Caso::Mochila => soma / 2.0,
    };
    (vizinhas, p_max)
}

/// Roda as decisoes da propria tomada com `n - 1` vizinhas: a linha do resumo e o maior tempo.
fn medir(n: usize, caso: Caso, radio: bool, rodada: u64) -> (String, i64) {
    let propria_w = if caso == Caso::Folgado { 2.0 } else { 1000.0 };
    let mut semente = rodada * 1_000_000 + (n as u64) * 1000 + caso as u64;
    let mut ciclo = Ciclo::novo(&Tomada { w_i: 10.0, ..Tomada::default() }, 0.0, 0.0, 0);
    let mut tempos = Vec::with_capacity(REPETICOES);
    let mut estado = "";
    // FNV-1a dos estados decididos: variantes de compilacao tem que decidir igual.
    let mut digest: u32 = 0x811c9dc5;
    for k in 0..REPETICOES as i64 {
        let (vizinhas, p_max) = instancia(n, caso, propria_w, &mut semente);
        let config = Tomada { w_i: 10.0, p_max: p_max as f32, ..Tomada::default() };
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
        "{} r{} {:<10} n={:>2} | {:>3} decisoes | mediana {:>8} us, IQR {:>8}-{:<8} us, p99 {:>8} us, max {:>8} us | estado {:<3} {:08x} | pilha usada ate aqui {:>5} B | heap livre minimo {:>6} B",
        if radio { "com-radio" } else { "sem-radio" },
        rodada,
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

/// Liga o Wi-Fi sem roteador e o ESP-NOW, e envia broadcast 10 vezes por segundo em CPU0.
fn ligar_radio() -> anyhow::Result<(EspWifi<'static>, EspNow<'static>)> {
    let p = Peripherals::take()?;
    // Sem NVS: nao toca nas credenciais gravadas na placa.
    let mut wifi = EspWifi::new(p.modem, EspSystemEventLoop::take()?, None)?;
    wifi.set_configuration(&Configuration::Client(ClientConfiguration::default()))?;
    wifi.start()?;
    let r = unsafe { sys::esp_wifi_set_channel(1, sys::wifi_second_chan_t_WIFI_SECOND_CHAN_NONE) };
    anyhow::ensure!(r == 0, "esp_wifi_set_channel: {r}");
    let espnow = EspNow::take()?;
    let mut peer: sys::esp_now_peer_info_t = unsafe { core::mem::zeroed() };
    peer.peer_addr = BROADCAST;
    peer.ifidx = sys::wifi_interface_t_WIFI_IF_STA;
    espnow.add_peer(peer)?;
    std::thread::Builder::new().stack_size(4096).spawn(|| loop {
        let quadro = [0u8; 44];
        unsafe { sys::esp_now_send(BROADCAST.as_ptr(), quadro.as_ptr(), quadro.len()) };
        std::thread::sleep(Duration::from_millis(100));
    })?;
    Ok((wifi, espnow))
}

fn main() {
    sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();
    // O log do Wi-Fi nao entra na medicao, e a captura para ao ler "F," na serial.
    unsafe { sys::esp_log_level_set(b"*\0".as_ptr() as *const _, sys::esp_log_level_t_ESP_LOG_ERROR) };
    // Como a tarefa principal do firmware. A rodada de 04/11/2026 nao fixava nucleo nem prioridade.
    ThreadSpawnConfiguration { pin_to_core: Some(Core::Core0), priority: 1, ..Default::default() }
        .set()
        .expect("configuracao das threads");

    let heap_antes = unsafe { sys::esp_get_free_heap_size() };
    let mac = {
        let mut m = [0u8; 6];
        unsafe { sys::esp_efuse_mac_get_default(m.as_mut_ptr()) };
        m.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":")
    };
    let cabecalho = format!(
        "=== bancada ampla | placa {mac} | CPU {} MHz | heap livre antes {heap_antes} B | pilha da thread {PILHA} B",
        unsafe { sys::esp_rom_get_cpu_ticks_per_us() }
    );
    println!("{cabecalho}");
    let mut linhas = Vec::new();
    let mut _radio = None;
    for (radio, rodada) in FASES {
        if radio {
            // Falha aqui nao pode reiniciar a placa: perderia as fases sem radio ja medidas.
            match ligar_radio() {
                Ok(r) => _radio = Some(r),
                Err(e) => {
                    let l = format!("radio nao ligou, fase pulada: {e:?}");
                    println!("{l}");
                    linhas.push(l);
                    continue;
                }
            }
        }
        println!("=== fase radio={radio} rodada={rodada}");
        // Thread com pilha conhecida: a marca d'agua dela e so a da decisao.
        let novas = std::thread::Builder::new()
            .stack_size(PILHA)
            .spawn(move || {
                let mut linhas = Vec::new();
                for caso in CASOS {
                    for n in TAMANHOS {
                        let (l, max) = medir(n, caso, radio, rodada);
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
        linhas.extend(novas);
    }

    loop {
        println!("{cabecalho}");
        for l in &linhas {
            println!("{l}");
        }
        println!("=== fim");
        std::thread::sleep(Duration::from_secs(10));
    }
}
