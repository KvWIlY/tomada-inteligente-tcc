//! Item 29 do parecer (2026-10-22): tres placas trocando heartbeats ESP-NOW de verdade, cada
//! uma no papel de uma tomada do cenario 6 (ferro, air fryer, micro-ondas num teto de 2.540 W).
//! Cada placa roda o `Ciclo::decidir` do firmware a cada 10 s, com fase aleatoria, sobre um
//! consumo sintetico (rotina de 6 min com os tres em uso juntos por 30 s) que le 0 W com o rele
//! aberto; o "usar agora" e apertado como no simulador. O ferro fica mudo de 1.200 a 1.290 s
//! para testar a vizinha sumida virando carga fixa. Termina em `DURACAO_S`.
//!
//! Do firmware: `Heartbeat` (v7, HMAC, anti-replay por `replay::aceitar`), `Ciclo`, `uso_agora`.
//! Copiado do `espnow.rs`: a tabela de vizinhas e o `montar_coordenacao`, aqui com contadores de
//! quadros recebidos e perdidos (o `contador` do heartbeat), que a tabela privada de la nao expoe.
//!
//! cd firmware && cargo build --example espnow_teste
//! espflash flash --port COMx --partition-table partitions.csv ../target/xtensa-esp32-espidf/debug/examples/espnow_teste
//! Para alinhar a rotina, as placas sao reiniciadas juntas (RTS) pelo PC que le as seriais.
//!
//! Teste longo (revisao do orientador 4.5), com variaveis de ambiente na compilacao:
//! `ESPNOW_TESTE_SSID`/`ESPNOW_TESTE_SENHA` conectam ao roteador da casa (o canal passa a ser o
//! dele e o radio fica no modem-sleep padrao, como no firmware), `ESPNOW_TESTE_DURACAO_S` troca os
//! 30 min. A cada minuto cada placa difunde as proprias contagens de recepcao (quadro 0xC7), e a
//! placa ligada ao PC imprime as das outras (linhas `C`): as placas nos outros comodos so precisam
//! de um carregador. `ok/revisao-orientador-2026-10-26/espnow-longo.py` resume a serial.

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

#[allow(dead_code, unused_imports)]
mod comunicacao {
    #[path = "../../src/comunicacao/espnow.rs"]
    pub mod espnow;
    /// O teste nao pareia: nenhum quadro e de pareamento.
    pub mod pareamento {
        pub fn talvez_encaminhar(_: &esp_idf_svc::espnow::ReceiveInfo, _: &[u8]) -> bool {
            false
        }
    }
    pub mod sntp {
        pub fn uptime_s() -> i64 {
            (unsafe { esp_idf_svc::sys::esp_timer_get_time() } / 1_000_000) as i64
        }
    }
    /// Sem NVS: o `boot_id` fica em 1. As placas reiniciam juntas, entao nenhuma vizinha
    /// guarda um boot anterior para recusar.
    pub mod wifi {
        pub fn nvs_particao() -> Option<esp_idf_svc::nvs::EspDefaultNvsPartition> {
            None
        }
    }
}

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use esp_idf_svc::espnow::{EspNow, ReceiveInfo};
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::sys;
use esp_idf_svc::wifi::{AuthMethod, ClientConfiguration, Configuration, EspWifi};

use comunicacao::espnow::{Heartbeat, BROADCAST};
use comunicacao::sntp::uptime_s;
use decisao::ciclo::Ciclo;
use persistencia::config::Tomada;
use sensores::potencia;
use solver::replay::{self, EstadoPeer};
use solver::uso_agora::{self, Origem};
use solver::{Estado, TomadaInput};

const CICLO_S: u64 = 10;
/// O `TTL_PEER_S` do `main.rs`: 3 ciclos.
const TTL_S: i64 = 30;
const PERIODO_S: i64 = 360;
const DURACAO_PADRAO_S: i64 = 1800;
const P_MAX: f32 = 2540.0;
/// Primeiro byte do quadro de contagens; o heartbeat comeca pela versao (7) e o recusa.
const QUADRO_CONTAGENS: u8 = 0xC7;
/// Chave de teste (nao e chave de casa nenhuma).
const CHAVE: [u8; 32] = [0x5a; 32];

struct Papel {
    mac: [u8; 6],
    nome: &'static str,
    uso_w: f32,
    espera_w: f32,
    w_i: f32,
    /// Em uso de `a` a `b` segundos dentro de cada periodo.
    uso: (i64, i64),
    mudo: Option<(i64, i64)>,
}

const PAPEIS: [Papel; 3] = [
    Papel { mac: [0xe8, 0x6b, 0xea, 0xd0, 0xad, 0xc0], nome: "ferro", uso_w: 1000.0, espera_w: 0.0, w_i: 3.0, uso: (20, 200), mudo: Some((1200, 1290)) },
    Papel { mac: [0xa8, 0x42, 0xe3, 0xa7, 0xdb, 0xc8], nome: "air-fryer", uso_w: 1500.0, espera_w: 0.5, w_i: 10.0, uso: (60, 240), mudo: None },
    Papel { mac: [0xa0, 0xa3, 0xb3, 0xab, 0x24, 0xac], nome: "microondas", uso_w: 1400.0, espera_w: 0.8, w_i: 10.0, uso: (120, 150), mudo: None },
];

struct Vizinha {
    hb: Heartbeat,
    recebido_em: i64,
    replay: EstadoPeer,
    recebidos: u32,
    perdidos: u32,
    /// Maior intervalo (s) entre dois heartbeats aceitos desta vizinha.
    maior_lacuna_s: u16,
}

fn tabela() -> &'static Mutex<BTreeMap<[u8; 6], Vizinha>> {
    static T: OnceLock<Mutex<BTreeMap<[u8; 6], Vizinha>>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Quadros de contagens recebidos das outras placas, impressos pelo laco principal (imprimir
/// no callback, que roda na tarefa do Wi-Fi, arrisca a pilha dela).
fn contagens_recebidas() -> &'static Mutex<Vec<([u8; 6], Vec<u8>)>> {
    static C: OnceLock<Mutex<Vec<([u8; 6], Vec<u8>)>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(Vec::new()))
}

/// `[0xC7, n, (mac, recebidos u32, perdidos u32, maior_lacuna_s u16) x n]`, little-endian.
fn quadro_contagens() -> Vec<u8> {
    let t = tabela().lock().unwrap();
    let mut q = vec![QUADRO_CONTAGENS, t.len() as u8];
    for (mac, v) in t.iter() {
        q.extend_from_slice(mac);
        q.extend_from_slice(&v.recebidos.to_le_bytes());
        q.extend_from_slice(&v.perdidos.to_le_bytes());
        q.extend_from_slice(&v.maior_lacuna_s.to_le_bytes());
    }
    q
}

/// Linhas `C,recebe,envia,recebidos,perdidos,maior_lacuna_s,uptime`: `recebe` e quem mandou o
/// quadro, `envia` a vizinha cujos heartbeats ele contou.
fn imprimir_contagens(de: &[u8; 6], q: &[u8], agora: i64) {
    for r in q.get(2..).unwrap_or(&[]).chunks_exact(16).take(q[1] as usize) {
        let u32_em = |i: usize| u32::from_le_bytes(r[i..i + 4].try_into().unwrap());
        let para: [u8; 6] = r[..6].try_into().unwrap();
        let lacuna = u16::from_le_bytes([r[14], r[15]]);
        println!("C,{},{},{},{},{},{}", mac_txt(de), mac_txt(&para), u32_em(6), u32_em(10), lacuna, agora);
    }
}

/// O `espnow::montar_coordenacao` do firmware: viva vira variavel, sumida vira carga fixa.
fn montar(agora: i64) -> (Vec<([u8; 6], TomadaInput)>, f32) {
    let mut vivas = Vec::new();
    let mut carga_fixa = 0.0f32;
    for (mac, v) in tabela().lock().unwrap().iter() {
        if agora - v.recebido_em <= TTL_S {
            vivas.push((*mac, v.hb.para_tomada_input()));
        } else {
            carga_fixa += v.hb.p_medida;
        }
    }
    (vivas, carga_fixa)
}

fn mac_txt(m: &[u8; 6]) -> String {
    m.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":")
}

fn main() -> anyhow::Result<()> {
    sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let p = Peripherals::take()?;
    let sysloop = EspSystemEventLoop::take()?;
    // Sem NVS: o teste nao toca nas credenciais gravadas na placa.
    let mut wifi = EspWifi::new(p.modem, sysloop, None)?;
    let duracao_s: i64 = option_env!("ESPNOW_TESTE_DURACAO_S").and_then(|s| s.parse().ok()).unwrap_or(DURACAO_PADRAO_S);
    if let Some(ssid) = option_env!("ESPNOW_TESTE_SSID") {
        let senha = option_env!("ESPNOW_TESTE_SENHA").unwrap_or("");
        wifi.set_configuration(&Configuration::Client(ClientConfiguration {
            ssid: ssid.try_into().map_err(|_| anyhow::anyhow!("SSID com mais de 32 bytes"))?,
            password: senha.try_into().map_err(|_| anyhow::anyhow!("senha com mais de 63 bytes"))?,
            auth_method: if senha.is_empty() { AuthMethod::None } else { AuthMethod::WPA2Personal },
            ..Default::default()
        }))?;
        wifi.start()?;
        wifi.connect()?;
        // Sem `esp_wifi_set_ps`: o radio fica no modem-sleep padrao, como no firmware.
        let inicio = uptime_s();
        while !wifi.is_connected()? {
            anyhow::ensure!(uptime_s() - inicio < 30, "roteador {ssid} nao conectou em 30 s");
            std::thread::sleep(Duration::from_millis(200));
        }
        println!("I,roteador,{ssid},conectado");
    } else {
        wifi.set_configuration(&Configuration::Client(ClientConfiguration::default()))?;
        wifi.start()?;
        let r = unsafe { sys::esp_wifi_set_channel(1, sys::wifi_second_chan_t_WIFI_SECOND_CHAN_NONE) };
        anyhow::ensure!(r == 0, "esp_wifi_set_channel: {r}");
    }

    let mut mac = [0u8; 6];
    unsafe { sys::esp_wifi_get_mac(sys::wifi_interface_t_WIFI_IF_STA, mac.as_mut_ptr()) };
    let Some(papel) = PAPEIS.iter().find(|x| x.mac == mac) else {
        loop {
            println!("E,placa {} sem papel no teste", mac_txt(&mac));
            std::thread::sleep(Duration::from_secs(5));
        }
    };

    let espnow = EspNow::take()?;
    espnow.register_recv_cb(|info: &ReceiveInfo, data: &[u8]| {
        if data.first() == Some(&QUADRO_CONTAGENS) && data.len() >= 2 {
            contagens_recebidas().lock().unwrap().push((*info.src_addr, data.to_vec()));
            return;
        }
        let Some(hb) = Heartbeat::desserializar_verificado(data, &CHAVE, info.src_addr) else { return };
        let mut t = tabela().lock().unwrap();
        let anterior = t.get(info.src_addr);
        let (veredito, novo) = replay::aceitar(anterior.map(|v| v.replay), hb.boot_id, hb.contador);
        let (Some(estado), true) = (novo, veredito.passou()) else { return };
        let (mut recebidos, mut perdidos) = anterior.map(|v| (v.recebidos, v.perdidos)).unwrap_or((0, 0));
        if veredito == replay::Veredito::Aceito {
            perdidos += hb.contador - anterior.map(|v| v.replay.ultimo_contador).unwrap_or(hb.contador) - 1;
        }
        recebidos += 1;
        let agora = uptime_s();
        let maior_lacuna_s = anterior
            .map(|v| v.maior_lacuna_s.max((agora - v.recebido_em).clamp(0, u16::MAX as i64) as u16))
            .unwrap_or(0);
        t.insert(*info.src_addr, Vizinha { hb, recebido_em: agora, replay: estado, recebidos, perdidos, maior_lacuna_s });
    })?;
    let mut peer: sys::esp_now_peer_info_t = unsafe { core::mem::zeroed() };
    peer.peer_addr = BROADCAST;
    peer.channel = 0;
    peer.ifidx = sys::wifi_interface_t_WIFI_IF_STA;
    peer.encrypt = false;
    espnow.add_peer(peer)?;

    let config = Tomada { w_i: papel.w_i, p_max: P_MAX, p_limiar_detectado_auto: Some(15.0), ..Tomada::default() };
    let mut ciclo = Ciclo::novo(&config, 0.0, 0.0, uptime_s());
    let mut rele_aberto = true;
    let mut janela = None;
    let (mut botoes, mut enviados, mut k) = (0u32, 0u32, 0u32);
    println!("I,{},{},inicio", papel.nome, mac_txt(&mac));
    // Fase aleatoria: na casa os ciclos de 10 s das tomadas nao andam juntos.
    std::thread::sleep(Duration::from_millis((unsafe { sys::esp_random() } % 10_000) as u64));

    while uptime_s() < duracao_s {
        let t = uptime_s();
        for (de, q) in contagens_recebidas().lock().unwrap().drain(..) {
            imprimir_contagens(&de, &q, t);
        }
        let fase = t % PERIODO_S;
        let demanda = if fase >= papel.uso.0 && fase < papel.uso.1 { papel.uso_w } else { papel.espera_w };
        let leitura_w = if rele_aberto { 0.0 } else { demanda };
        if uso_agora::expirou(janela, t) {
            janela = None;
        }
        if rele_aberto && demanda > 15.0 && janela.is_none() {
            janela = Some(uso_agora::abrir(t, Origem::Botao));
            ciclo.marcar_uso_agora(t);
            botoes += 1;
        }
        let efeito = uso_agora::efeito(janela, false, t);
        let (vivas, carga_fixa) = montar(t);
        let t0 = unsafe { sys::esp_timer_get_time() };
        let d = ciclo.decidir(&config, potencia::leitura(0.0, 0.0, leitura_w), &vivas, carga_fixa, mac, None, efeito, 12, t);
        let decidir_ms = (unsafe { sys::esp_timer_get_time() } - t0) / 1000;
        rele_aberto = d.estado == Estado::Off;

        let mudo = papel.mudo.is_some_and(|(a, b)| t >= a && t < b);
        if !mudo {
            let hb = Heartbeat::novo(0, d.p_medida, d.estado.idx() as u8, d.u_i, d.f_i, false, true, d.cstd, d.con, config.w_efetivo());
            if espnow.send(BROADCAST, &hb.serializar_assinado(&CHAVE, &mac)).is_ok() {
                enviados += 1;
            }
        }
        println!(
            "L,{},{},{},{:.0},{:.0},{},{},{},{},{:.0},{},{}",
            papel.nome, t, k, demanda, leitura_w, d.estado.as_str(), d.u_i as u8, d.f_i as u8,
            vivas.len(), carga_fixa, decidir_ms, !mudo as u8
        );
        if k % 6 == 5 {
            let q = quadro_contagens();
            let _ = espnow.send(BROADCAST, &q);
            imprimir_contagens(&mac, &q, t);
        }
        k += 1;
        std::thread::sleep(Duration::from_secs(CICLO_S));
    }

    loop {
        println!("S,{},{},enviados,{},botoes,{}", papel.nome, mac_txt(&mac), enviados, botoes);
        for (m, v) in tabela().lock().unwrap().iter() {
            println!("R,{},{},recebidos,{},perdidos,{}", papel.nome, mac_txt(m), v.recebidos, v.perdidos);
        }
        println!("F,{}", papel.nome);
        std::thread::sleep(Duration::from_secs(5));
    }
}
