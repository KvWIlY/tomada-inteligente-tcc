//! Tomada inteligente — orquestracao do ciclo medir -> decidir -> atuar -> sincronizar.
//!
//! Boot (005): LittleFS -> identidade/PIN -> Wi-Fi (portal AP se preciso) -> SNTP
//! NAO-BLOQUEANTE -> ADC/rele/ESP-NOW. NADA no boot espera por internet: sem hora
//! certa, sem servidor ou sem rede alguma, a tomada entra no ciclo local do mesmo
//! jeito (FR-001/004, Principio VI).
//!
//! Loop (cadencia 10 s, SC-008):
//!   medir (RMS V/I) -> heartbeats ESP-NOW v5 -> decidir (MILP) -> atuar rele.
//! Sem NENHUM peer vivo e online (003): irmas lidas do backend viram carga fixa.
//! A cada ~60 s (cloud tick, so no modo nuvem): bufferiza medicao, `GET /config`
//! (config + rele manual + P_max + chave da casa) e `POST /telemetria` em lote.
//!
//! Modo standalone: identidade local, sem claim-code e sem I/O de nuvem — a tomada
//! coordena com as irmas por ESP-NOW usando a chave recebida no pareamento local.

use anyhow::Result;
use chrono::{Local, Timelike};
use esp_idf_svc::hal::adc::attenuation::DB_12;
use esp_idf_svc::hal::adc::oneshot::config::AdcChannelConfig;
use esp_idf_svc::hal::adc::oneshot::{AdcChannelDriver, AdcDriver};
use esp_idf_svc::hal::gpio::{PinDriver, Pull};
use esp_idf_svc::hal::peripherals::Peripherals;
use std::thread;
use std::time::Duration;

mod comunicacao;
mod config_nuvem;
mod decisao;
mod persistencia;
mod sensores;

use comunicacao::espnow::{self, Heartbeat};
use comunicacao::pareamento as pair;
use comunicacao::sntp;
use comunicacao::wifi::{ContextoBoot, ResultadoWifi};
use comunicacao::wifi_provisioning::CredsRecebidas;
use comunicacao::{backend, provisionamento};
use decisao::atuador::Atuador;
use decisao::ciclo::Ciclo;
use persistencia::config;
use persistencia::credenciais::{self, CredenciaisLocais};
use persistencia::medicao::{self, Medicao};
use persistencia::retencao;
use tomada_solver::coordenacao::{carga_fixa_nuvem, contar_frescas, RemoteTomada};
use tomada_solver::portal::{Modo, MotivoPortal, ResolucaoWifi};
use tomada_solver::relogio;
use tomada_solver::sync::deve_adotar_config;
use tomada_solver::uso_agora;

/// Cadencia do ciclo de decisao (s) — SC-008 (decisao em <=10 s).
const CICLO_S: u64 = 10;
/// A cada quantos ciclos fazemos I/O de nuvem (~60 s => medicao 1/min).
const CICLOS_POR_NUVEM: u64 = 6;
/// TTL de um peer ESP-NOW, em UPTIME local (nao epoch) — 3x a cadencia.
const TTL_PEER_S: i64 = (CICLO_S as i64) * 3;
/// Janela de frescor do fallback de coordenacao pela nuvem (003).
const JANELA_FRESCOR_S: i64 = 300;
/// Falhas transitorias consecutivas de provisionamento antes de reabrir o portal
/// com a causa "servidor inacessivel" (FR-019a). 10 x 15 s ~ 2,5 min.
const MAX_FALHAS_SERVIDOR: u32 = 10;

fn main() -> Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    // Fuso horario (POSIX TZ). So tem efeito depois que a hora certa chega.
    std::env::set_var("TZ", "<-03>3");
    unsafe { esp_idf_svc::sys::tzset() };

    log::info!("=== tomada inteligente: boot ===");

    // 1. LittleFS
    persistencia::init()?;

    // 2. Identidade que JA existe (pode nao existir ainda) + PIN de pareamento.
    let mut creds_existentes = credenciais::load()?;
    if let Some(c) = creds_existentes.as_mut() {
        credenciais::garantir_pin(c)?;
    }
    let pin_proprio = creds_existentes
        .as_ref()
        .map(|c| c.pin.clone())
        .unwrap_or_else(credenciais::gerar_pin);
    let modo_standalone = creds_existentes
        .as_ref()
        .map(|c| c.eh_standalone())
        .unwrap_or(false);

    // 3. Wi-Fi. Pode devolver um formulario do portal para aplicarmos (e reiniciar).
    let peripherals = Peripherals::take()?;
    let ctx = ContextoBoot {
        tem_identidade_nuvem: creds_existentes
            .as_ref()
            .map(|c| !c.eh_standalone())
            .unwrap_or(false),
        pin_proprio: pin_proprio.clone(),
        modo_standalone_sem_rede: modo_standalone,
    };
    let _wifi = match comunicacao::wifi::conectar(peripherals.modem, ctx)? {
        ResultadoWifi::Pronto(h) => h,
        ResultadoWifi::Configurar(creds) => {
            aplicar_formulario(*creds, &pin_proprio)?; // nao retorna (reinicia)
            unreachable!()
        }
    };

    // 4. SNTP em segundo plano — NUNCA bloqueia nem falha o boot (FR-001).
    //    Ate a hora chegar, `sntp::agora_epoch()` devolve uptime e as regras horarias
    //    ficam neutras (FR-001b).
    let sntp_handle = sntp::iniciar();
    // Medicoes sem hora sobradas de um boot que nunca sincronizou sao incorrigiveis.
    let descartadas = medicao::descartar_sem_hora_de_boot_anterior().unwrap_or(0);
    if descartadas > 0 {
        backend::registrar_evento(
            "relogio_descartes",
            "warn",
            "Medicoes sem hora certa descartadas",
            &format!("n={descartadas}"),
        );
    }

    // 5. Identidade de regime: standalone opera direto; nuvem provisiona (com portal
    //    de volta se o claim for ruim ou o servidor nao responder).
    let creds = match creds_existentes {
        Some(c) => c,
        None => obter_identidade()?,
    };
    log::info!(
        "[BOOT] tomada_id={} modo={:?} PIN={}",
        creds.tomada_id,
        creds.modo,
        creds.pin
    );

    // 6. ADC: GPIO32 = tensao (ZMPT101B), GPIO35 = corrente (SCT-013).
    let adc1 = AdcDriver::new(peripherals.adc1)?;
    let ch_cfg = AdcChannelConfig {
        attenuation: DB_12,
        ..Default::default()
    };
    let mut ch_v = AdcChannelDriver::new(&adc1, peripherals.pins.gpio32, &ch_cfg)?;
    let mut ch_i = AdcChannelDriver::new(&adc1, peripherals.pins.gpio35, &ch_cfg)?;

    // 7. Rele (GPIO26, Opcao A). Comeca aberto (OFF) — mesmo estado do portal (FR-003a).
    let mut atuador = Atuador::novo(peripherals.pins.gpio26)?;

    // 8. Botao BOOT (GPIO0): abre a janela de pareamento de 60 s (FR-008).
    //    Pull-up interno => nivel baixo quando pressionado.
    let botao = PinDriver::input(peripherals.pins.gpio0, Pull::Up)?;

    // 8b. Botao "USAR AGORA" (GPIO27, dedicado): religa a tomada que o MILP cortou.
    //     Botao SEPARADO do BOOT de proposito — nada de toque curto/longo para o dono
    //     decorar. E a UNICA porta de religar no modo standalone, onde nao ha dashboard.
    //     Mesmo esquema eletrico do BOOT: pull-up interno, botao para o GND.
    let botao_usar = PinDriver::input(peripherals.pins.gpio27, Pull::Up)?;

    // 9. ESP-NOW (best-effort; exige a chave da casa — Principio VIII, fail-closed).
    let espnow = match parse_chave_hex(&creds.residencia_key) {
        Some(key) => match espnow::iniciar(key) {
            Ok(e) => Some(e),
            Err(e) => {
                log::warn!("[BOOT] ESP-NOW indisponivel ({e}); operando sozinho");
                None
            }
        },
        None => {
            log::warn!("[BOOT] sem residencia_key valida -> ESP-NOW DESABILITADO. Operando sozinho.");
            None
        }
    };
    // MAC proprio: identidade de hardware e chave de ordenacao canonica da coordenacao.
    let proprio_mac = provisionamento::obter_mac_bytes().unwrap_or([0u8; 6]);

    // 9b. Pareamento pendente: o portal recebeu o PIN da tomada existente e mandamos
    //     reiniciar; agora, com o radio no ar, buscamos a chave da casa (FR-008).
    let mut creds = creds;
    if let (Some(pin_alvo), Some(en)) = (credenciais::ler_pin_alvo(), espnow.as_ref()) {
        log::warn!("[PAIR] pareamento pendente -- procurando a outra tomada...");
        match pair::parear_como_nova(en, &pin_alvo, &proprio_mac) {
            Ok(chave) => {
                let hex = hex_de(&chave);
                if let Err(e) = credenciais::salvar_residencia_key(&hex) {
                    log::error!("[PAIR] falha gravando a chave recebida: {e:?}");
                } else {
                    creds.residencia_key = hex;
                    espnow::definir_chave(chave);
                    credenciais::limpar_pin_alvo();
                    backend::registrar_evento(
                        "pareamento_ok",
                        "info",
                        "Pareada com a tomada existente",
                        "origem=portal",
                    );
                    log::warn!("[PAIR] PAREAMENTO CONCLUIDO -- coordenando com as irmas");
                }
            }
            Err(f) => {
                credenciais::limpar_pin_alvo(); // nao insiste sozinha
                log::error!("[PAIR] falhou: {}", f.mensagem());
                backend::registrar_evento(
                    "pareamento_falha",
                    "warn",
                    "Pareamento local falhou",
                    f.as_str(),
                );
            }
        }
    }
    let creds = creds;
    let chave_casa = parse_chave_hex(&creds.residencia_key);

    // 10. Estado do ciclo
    let mut config = config::load()?;
    let agora0 = provisionamento::agora_epoch();
    let mut ciclo = Ciclo::novo(&config, 0.0, 0.0, agora0);
    let short_id = short_id(&creds.tomada_id);
    let mut rele_manual: Option<bool> = None;
    let mut carga_nuvem: Option<f32> = None;
    // Janela de "usar agora" vigente (botao fisico ou dashboard). Vive so em RAM: um
    // reboot devolve a decisao ao MILP, que e o padrao seguro.
    let mut janela_uso: Option<uso_agora::Janela> = None;
    // O `GET /config` repete `Some(false)` ate o servidor ser liberado; sem esta marca
    // cada tick de nuvem reabriria a janela para sempre.
    let mut usar_agora_consumido = false;
    // Pedido pendente de devolver `rele_aberto` a NULL (automatico) no servidor.
    let mut liberar_manual = false;

    log::info!("[BOOT] entrando no loop principal (ciclo={CICLO_S}s)");

    let mut n: u64 = 0;
    loop {
        // --- RELOGIO: na transicao para hora certa, corrige os carimbos pendentes ---
        // PRECISA vir antes de ler `agora`/`hora`: na iteracao em que a hora chega,
        // `relogio_ok()` passa a valer true mas `agora_epoch()` lido ANTES daqui ainda
        // devolve o uptime (~38 s) — e a medicao do tick era carimbada em 1970-01-01,
        // ia para o balde do dia 0 e, por nao ser a sentinela SEM_HORA, ainda subia
        // para o backend como um ponto valido.
        if let Some(off) = sntp::checar_sync(&sntp_handle) {
            aplicar_hora_certa(off, "sntp");
        }

        // Leituras de tempo do ciclo — todas DEPOIS do `checar_sync` acima.
        let agora = provisionamento::agora_epoch();
        let uptime = sntp::uptime_s();
        let hora = Local::now().hour() as u8;
        let h = (hora % 24) as usize;
        let cloud_tick = n % CICLOS_POR_NUVEM == 0;

        // --- BOTAO: janela de pareamento (borda de descida, com pull-up) ---
        if botao.is_low() {
            pair::abrir_janela();
            thread::sleep(Duration::from_millis(400)); // debounce
        }

        // --- BOTAO "USAR AGORA": religa o que o MILP cortou ---
        if botao_usar.is_low() {
            abrir_janela_uso(
                &mut janela_uso,
                &mut ciclo,
                uso_agora::Origem::Botao,
                uptime,
                agora,
            );
            thread::sleep(Duration::from_millis(400)); // debounce
        }

        // --- FIM DA JANELA: a decisao volta pro MILP (transicao, uma vez so) ---
        if uso_agora::expirou(janela_uso, uptime) {
            let origem = janela_uso.map(|j| j.origem.as_str()).unwrap_or("?");
            janela_uso = None;
            // No modo nuvem o comando precisa ser desfeito no servidor, senao o
            // proximo GET reabriria a janela e a tomada nunca voltaria ao automatico.
            liberar_manual = !creds.eh_standalone();
            // `usar_agora_consumido` NAO e limpo aqui de proposito: o servidor ainda
            // devolve `Some(false)` ate o POST de liberacao passar, e limpar agora
            // faria o proximo GET reabrir a janela que acabou de fechar — de novo, e
            // de novo, se o POST estiver falhando. Quem limpa e o proprio GET, quando
            // enfim trouxer outro valor.
            backend::registrar_evento(
                "usar_agora_fim",
                "info",
                "Janela de uso manual encerrada",
                &format!("origem={origem} duracao_s={}", uso_agora::DURACAO_S),
            );
        }

        // Corte manual do dashboard vence a janela; fora isso, a janela manda.
        let efeito = uso_agora::efeito(janela_uso, rele_manual == Some(true), uptime);

        // --- MEDIR (RMS de V e I) ---
        let (leitura, diag) = sensores::medir(
            || ch_v.read().unwrap_or(0) as f32,
            || ch_i.read().unwrap_or(0) as f32,
        );
        log::info!(
            "[DIAG] V_dc={:.0}mV V_ac={:.1}mV | I_dc={:.0}mV I_ac={:.2}mV",
            diag.v_dc_mv, diag.v_ac_mv, diag.i_dc_mv, diag.i_ac_mv,
        );

        // --- PAREAMENTO (so enquanto a janela estiver aberta) ---
        if let (Some(en), Some(key)) = (espnow.as_ref(), chave_casa) {
            pair::atender(en, &creds.pin, &proprio_mac, &key);
        }

        // --- COORDENACAO ESP-NOW (TTL por UPTIME local, nao por epoch) ---
        let (peers_vivos, carga_fixa) = espnow
            .as_ref()
            .map(|_| espnow::montar_coordenacao(uptime, TTL_PEER_S))
            .unwrap_or((Vec::new(), 0.0));

        // --- FALLBACK DE COORDENACAO PELA NUVEM (003) ---
        if !peers_vivos.is_empty() {
            carga_nuvem = None;
        } else if cloud_tick && !creds.eh_standalone() {
            carga_nuvem = if comunicacao::wifi::esta_conectado() {
                match backend::buscar_consumo_irmas() {
                    Ok(remotas) => {
                        let irmas: Vec<RemoteTomada> = remotas
                            .into_iter()
                            .map(|t| RemoteTomada {
                                tomada_id: t.id,
                                p_medida_atual: t.p_medida_atual,
                                ultimo_uso_ativo_ts: t.ultimo_uso_ativo_ts,
                            })
                            .collect();
                        let carga =
                            carga_fixa_nuvem(&creds.tomada_id, agora, JANELA_FRESCOR_S, &irmas);
                        if carga > 0.0 {
                            let frescas =
                                contar_frescas(&creds.tomada_id, agora, JANELA_FRESCOR_S, &irmas);
                            let total =
                                irmas.iter().filter(|t| t.tomada_id != creds.tomada_id).count();
                            backend::registrar_evento(
                                "coordenacao_nuvem",
                                "info",
                                "Fallback de coordenacao pela nuvem",
                                &format!("carga_fixa={carga:.1}W irmas_frescas={frescas}/{total}"),
                            );
                        }
                        Some(carga)
                    }
                    Err(e) => {
                        log::warn!("[NUVEM] fallback coordenacao: GET falhou ({e})");
                        None
                    }
                }
            } else {
                None
            };
        }
        let carga_fixa = match carga_nuvem {
            Some(nuvem) => carga_fixa.max(nuvem),
            None => carga_fixa,
        };

        // --- REGRAS HORARIAS: neutras enquanto nao ha hora certa (FR-001b) ---
        let rh = relogio::regras_horarias(sntp::relogio_ok(), config.schedule[h], config.pi_historico[h]);
        let mut config_ciclo = config.clone();
        config_ciclo.schedule[h] = rh.a_it;
        config_ciclo.pi_historico[h] = rh.pi;

        // --- DECIDIR (MILP) ---
        let decisao = ciclo.decidir(
            &config_ciclo,
            leitura,
            &peers_vivos,
            carga_fixa,
            proprio_mac,
            rele_manual,
            efeito,
            hora,
            agora,
        );

        // --- ATUAR ---
        if let Err(e) = atuador.aplicar(decisao.estado) {
            log::error!("[CICLO] falha ao atuar rele: {e:?}");
        }
        log::info!(
            "[CICLO] V={:.1} I={:.2} P={:.1}W estado={} rele_aberto={} peers={} carga_fixa={:.0} hora_ok={} usar_agora={}s",
            leitura.vrms, leitura.irms, leitura.p, decisao.estado.as_str(),
            decisao.rele_aberto, peers_vivos.len(), carga_fixa, sntp::relogio_ok(),
            janela_uso.map(|j| j.restante_s(uptime)).unwrap_or(0),
        );

        // --- DIFUNDIR HEARTBEAT (v5: boot_id + contador) ---
        if let Some(en) = &espnow {
            let hb = Heartbeat::novo(
                short_id,
                decisao.p_medida,
                decisao.estado.idx() as u8,
                decisao.u_i,
                decisao.f_i,
                rele_manual.unwrap_or(false),
                rh.a_it,
                decisao.cstd,
                decisao.con,
                rh.pi,
                config.w_efetivo(),
            );
            let _ = espnow::difundir(en, &hb, &proprio_mac);
        }

        for e in &decisao.eventos {
            backend::registrar_evento(e.tipo, e.severidade, &e.titulo, &e.detalhes);
        }

        // --- NUVEM (a cada ~60 s; nada disso no modo standalone — FR-006) ---
        if cloud_tick {
            // Sem hora certa, o carimbo fica com a sentinela e e corrigido depois.
            let ts = if sntp::relogio_ok() { agora } else { relogio::SEM_HORA };
            medicao::registrar(Medicao::novo(
                &creds.tomada_id,
                ts,
                decisao.p_medida,
                decisao.estado,
                decisao.rele_aberto,
            ));

            // Retencao ANTES de gravar: e o gatilho de espaco que garante disco para
            // o flush logo abaixo. Na ordem inversa, o flush e quem falhava.
            aplicar_retencao(agora);

            // Persiste o buffer de RAM. No modo nuvem o sync tambem faria isso, mas no
            // standalone ninguem faria — a medicao morreria no reboot.
            if let Err(e) = medicao::flush_para_disco() {
                log::error!("[MED] flush falhou: {e:?}");
            }

            if !creds.eh_standalone() {
                sincronizar_nuvem(
                    &mut config,
                    &mut rele_manual,
                    &decisao,
                    chave_casa,
                    &mut liberar_manual,
                );
                // "Usar agora" pedido pelo dashboard: o servidor manda `Some(false)` e
                // o firmware o transforma numa janela igual a do botao fisico — as duas
                // portas, um comportamento so.
                if uso_agora::deve_abrir_do_dashboard(rele_manual, usar_agora_consumido) {
                    usar_agora_consumido = true;
                    abrir_janela_uso(
                        &mut janela_uso,
                        &mut ciclo,
                        uso_agora::Origem::Dashboard,
                        uptime,
                        agora,
                    );
                }
                if rele_manual != Some(false) {
                    usar_agora_consumido = false; // comando novo podera abrir outra
                }
            }

            espnow::esquecer_antigos(uptime, TTL_PEER_S * 10);
        }

        n = n.wrapping_add(1);
        thread::sleep(Duration::from_secs(CICLO_S));
    }
}

/// Abre (ou renova) a janela de "usar agora" e registra o evento.
///
/// Re-ancora tambem o `ultimo_uso_ativo_ts` do ciclo: apertar o botao E uso, e sem
/// isso o `F_i` voltaria a valer no instante em que a janela fechasse, cortando a
/// tomada no mesmo ciclo em que ela deveria voltar ao automatico.
fn abrir_janela_uso(
    janela: &mut Option<uso_agora::Janela>,
    ciclo: &mut Ciclo,
    origem: uso_agora::Origem,
    uptime: i64,
    agora: i64,
) {
    *janela = Some(uso_agora::abrir(uptime, origem));
    ciclo.marcar_uso_agora(agora);
    log::warn!(
        "[USAR] janela aberta por {} ({} s) -- rele fecha se couber no P_max",
        origem.as_str(),
        uso_agora::DURACAO_S
    );
    backend::registrar_evento(
        "usar_agora",
        "info",
        "Religada a pedido do dono",
        &format!("origem={} duracao_s={}", origem.as_str(), uso_agora::DURACAO_S),
    );
}

/// Roda a retencao e transforma o resultado em observabilidade.
///
/// Purga por espaco em balde ja confirmado e rotina (o dado esta no banco) e nao vira
/// evento. Os dois casos que o dono precisa ver sao: apagamos medicao que NUNCA subiu,
/// e a purga nao conseguiu liberar o que precisava.
fn aplicar_retencao(agora: i64) {
    let r = match retencao::talvez_purgar(agora) {
        Ok(r) => r,
        Err(e) => {
            log::warn!("[NUVEM] retencao falhou: {e:?}");
            return;
        }
    };

    if r.baldes_com_perda > 0 {
        backend::registrar_evento(
            "retencao_perda",
            "warn",
            "Medicoes nao sincronizadas apagadas por falta de espaco",
            &format!(
                "baldes={} bytes_liberados={}",
                r.baldes_com_perda, r.bytes_liberados
            ),
        );
    }
    if r.sem_espaco {
        backend::registrar_evento(
            "retencao_sem_espaco",
            "erro",
            "Purga nao liberou espaco suficiente",
            &format!(
                "removidos={} bytes_liberados={}",
                r.por_espaco, r.bytes_liberados
            ),
        );
    }
    if r.por_idade > 0 || r.por_espaco > 0 {
        log::info!(
            "[RETENCAO] passada: {} balde(s) por idade, {} por espaco ({} B)",
            r.por_idade, r.por_espaco, r.bytes_liberados
        );
    }
}

/// Correcao dos carimbos pendentes na transicao para hora certa (FR-001a).
///
/// Extraida porque agora ha DUAS fontes de relogio — o SNTP publico e, para a casa
/// sem internet, a hora do backend da LAN. Ambas entregam o mesmo `offset` e devem
/// disparar exatamente a mesma correcao; duplicar isso seria a forma obvia de as duas
/// divergirem com o tempo.
fn aplicar_hora_certa(off: i64, fonte: &str) {
    // Eventos primeiro: os que ja estao na fila foram registrados sem hora e
    // precisam ser datados ANTES do proximo POST, senao subiriam em 1970.
    let ev = backend::corrigir_carimbos_eventos(off);
    match medicao::corrigir_carimbos(off) {
        Ok((corrigidos, descartados)) => backend::registrar_evento(
            "relogio_corrigido",
            "info",
            "Hora certa obtida; carimbos ajustados",
            &format!(
                "fonte={fonte} offset={off} corrigidos={corrigidos} descartados={descartados} eventos={ev}"
            ),
        ),
        Err(e) => log::warn!("[RELOGIO] correcao falhou: {e:?} (eventos={ev} datados)"),
    }
}

/// Pull do desired state + push do lote (modo nuvem).
fn sincronizar_nuvem(
    config: &mut persistencia::config::Tomada,
    rele_manual: &mut Option<bool>,
    decisao: &decisao::ciclo::Decisao,
    chave_atual: Option<[u8; 32]>,
    liberar_manual: &mut bool,
) {
    match backend::buscar_desired_state() {
        Ok(ds) => {
            // Fonte de relogio da LAN — so age se o SNTP ainda nao sincronizou
            // (`adotar_hora_do_servidor` e no-op depois disso). E o que mantem as
            // regras horarias do MILP vivas numa casa sem internet.
            if let Some(ts) = ds.agora_servidor {
                if let Some(off) = sntp::adotar_hora_do_servidor(ts) {
                    aplicar_hora_certa(off, "backend");
                }
            }
            *rele_manual = ds.rele_aberto;
            if let Some(mut remota) = ds.config {
                if deve_adotar_config(remota.config_version, config.config_version) {
                    remota.p_max = config.p_max;
                    if let Err(e) = config::save(&remota) {
                        log::warn!("[NUVEM] falha salvando config remota: {e:?}");
                    } else {
                        log::info!(
                            "[NUVEM] config adotada v{} -> v{}",
                            config.config_version, remota.config_version
                        );
                        *config = remota;
                    }
                }
            }
            if let Some(pmax) = ds.p_max {
                if (pmax - config.p_max).abs() > 0.5 {
                    log::info!("[NUVEM] P_max da residencia: {:.0} -> {:.0} W", config.p_max, pmax);
                    config.p_max = pmax;
                    if let Err(e) = config::save(config) {
                        log::warn!("[NUVEM] falha persistindo P_max: {e:?}");
                    }
                }
            }
            // Rotacao da chave da casa apos revogacao de um dispositivo (FR-012).
            if let Some(hex) = ds.residencia_key.as_deref() {
                if let Some(nova) = parse_chave_hex(hex) {
                    if Some(nova) != chave_atual {
                        if let Err(e) = credenciais::salvar_residencia_key(hex) {
                            log::error!("[NUVEM] falha persistindo chave nova: {e:?}");
                        } else {
                            espnow::definir_chave(nova);
                            backend::registrar_evento(
                                "chave_rotacionada",
                                "warn",
                                "Chave da residencia rotacionada",
                                "origem=revogacao de dispositivo",
                            );
                        }
                    }
                }
            }
        }
        Err(e) => log::warn!("[NUVEM] pull desired_state falhou: {e:?}"),
    }

    backend::registrar_snapshot(
        decisao.p_medida,
        decisao.ultimo_uso_ativo_ts,
        decisao.estado.as_str(),
        decisao.cstd,
        decisao.con,
        *liberar_manual,
    );

    match backend::sincronizar() {
        // So limpa a marca depois do 2xx: se o POST falhou, o servidor ainda tem o
        // comando velho e o pedido de liberacao precisa sobreviver ate a proxima volta.
        Ok(k) => {
            if *liberar_manual {
                *liberar_manual = false;
                *rele_manual = None; // ja e automatico localmente; o GET confirma depois
                log::info!("[NUVEM] comando manual liberado -- de volta ao automatico");
            }
            if k > 0 {
                log::info!("[NUVEM] {k} medicoes sincronizadas");
            }
        }
        Err(e) => log::warn!("[NUVEM] sync falhou: {e:?}"),
    }
}

/// Aplica o formulario do Portal AP e reinicia (o boot seguinte usa o que gravamos).
fn aplicar_formulario(creds: CredsRecebidas, pin: &str) -> Result<()> {
    // Wi-Fi primeiro: vale para todos os modos.
    match &creds.wifi {
        ResolucaoWifi::Gravar { ssid, senha } => comunicacao::wifi::salvar_wifi(ssid, senha)?,
        ResolucaoWifi::TrocarSenha { senha } => comunicacao::wifi::salvar_senha_nvs(senha)?,
        ResolucaoWifi::Preservar | ResolucaoWifi::SemRede => {}
    }

    match creds.form.modo {
        Modo::Nuvem => {
            config_nuvem::salvar_servidor(&creds.form.servidor)?;
            // PROMOCAO standalone -> nuvem. Sem descartar a identidade local, o boot
            // seguinte acha `cred.json`, devolve as credenciais standalone e NUNCA
            // chama `obter_identidade()` (main.rs: `Some(c) => c`): o claim ficaria
            // parado na NVS para sempre e a tomada jamais apareceria no dashboard.
            // As medicoes vao junto — foram gravadas com o `tomada_id` local e nunca
            // confirmariam sob a identidade nova, travando o sync (mesmo motivo
            // documentado em `medicao::limpar_tudo`).
            if credenciais::load()?.map(|c| c.eh_standalone()).unwrap_or(false) {
                log::warn!(
                    "[PROV] promovendo standalone -> nuvem: identidade local e medicoes descartadas"
                );
                credenciais::limpar_tudo();
                medicao::limpar_tudo();
            }
            credenciais::salvar_claim_pendente(&creds.form.claim)?;
            log::info!("[PROV] modo nuvem: claim salvo, provisionando apos o restart");
        }
        Modo::Standalone => {
            // Esta tomada VIRA a residencia: cunha o proprio id e a chave da casa.
            let mac = provisionamento::obter_mac()?;
            let c = CredenciaisLocais::nova_standalone(
                &mac,
                uuid_local(),
                credenciais::gerar_hex_32(),
                pin.to_string(),
            );
            credenciais::save(&c)?;
            credenciais::limpar_claim_pendente();
            log::warn!("[PROV] modo STANDALONE: identidade local criada (sem nuvem)");
        }
        Modo::Parear => {
            // Parear = entrar numa casa que ja existe, sem nuvem. Se esta tomada ainda
            // nao tem identidade, cria uma local agora (a chave da casa e substituida
            // pela recebida no handshake).
            if credenciais::load()?.is_none() {
                let mac = provisionamento::obter_mac()?;
                let c = CredenciaisLocais::nova_standalone(
                    &mac,
                    uuid_local(),
                    credenciais::gerar_hex_32(),
                    pin.to_string(),
                );
                credenciais::save(&c)?;
            }
            // O handshake exige o radio no ar: acontece no proximo boot.
            credenciais::salvar_pin_alvo(&creds.form.pin)?;
            log::warn!("[PROV] modo PAREAR: buscando a outra tomada apos o restart");
        }
    }

    log::info!("[WIFI] configuracao salva. Reiniciando em 4s...");
    thread::sleep(Duration::from_secs(4));
    unsafe { esp_idf_svc::sys::esp_restart() }
}

/// Identidade de regime no modo nuvem: provisiona com o claim do portal, tratando
/// claim ruim (reabre portal) e servidor fora do ar (FR-019a).
fn obter_identidade() -> Result<CredenciaisLocais> {
    let mut falhas_servidor: u32 = 0;
    loop {
        match obter_ou_provisionar()? {
            Provisao::Pronto(c) => return Ok(c),
            Provisao::ReabrirPortal => {
                comunicacao::wifi::forcar_portal_e_reiniciar(MotivoPortal::ClaimInvalido)
            }
            Provisao::Retentar => {
                falhas_servidor += 1;
                if falhas_servidor >= MAX_FALHAS_SERVIDOR {
                    log::error!(
                        "[PROV] servidor inacessivel apos {falhas_servidor} tentativas -- reabrindo portal"
                    );
                    comunicacao::wifi::forcar_portal_e_reiniciar(
                        MotivoPortal::ServidorInacessivel,
                    );
                }
                log::warn!(
                    "[PROV] nova tentativa em 15s ({falhas_servidor}/{MAX_FALHAS_SERVIDOR})..."
                );
                thread::sleep(Duration::from_secs(15));
            }
        }
    }
}

/// Resultado de uma tentativa de provisionamento.
enum Provisao {
    Pronto(CredenciaisLocais),
    ReabrirPortal,
    Retentar,
}

fn obter_ou_provisionar() -> Result<Provisao> {
    if let Some(c) = credenciais::load()? {
        return Ok(Provisao::Pronto(c));
    }
    let mac = provisionamento::obter_mac()?;

    let claim = match credenciais::ler_claim_pendente() {
        Some(c) if !c.is_empty() => c,
        _ => {
            log::warn!("[PROV] sem claim-code pendente -- abrindo portal p/ o dono digitar");
            return Ok(Provisao::ReabrirPortal);
        }
    };

    log::info!("[PROV] provisionando MAC={mac} com claim-code do portal...");
    match provisionamento::provisionar(&mac, &claim) {
        Ok(creds) => {
            credenciais::limpar_claim_pendente();
            Ok(Provisao::Pronto(creds))
        }
        Err(provisionamento::ErroProv::ClaimInvalido) => {
            log::error!("[PROV] claim invalido/expirado/usado -- descartando e reabrindo portal");
            credenciais::limpar_claim_pendente();
            Ok(Provisao::ReabrirPortal)
        }
        Err(provisionamento::ErroProv::Transitorio(e)) => {
            log::warn!("[PROV] provisionamento falhou: {e}");
            Ok(Provisao::Retentar)
        }
    }
}

/// UUID v4 gerado localmente (modo standalone; o servidor nunca ve este id).
fn uuid_local() -> String {
    let h = credenciais::gerar_hex_32(); // 64 chars
    format!(
        "{}-{}-4{}-a{}-{}",
        &h[0..8],
        &h[8..12],
        &h[13..16],
        &h[17..20],
        &h[20..32]
    )
}

/// 32 bytes -> 64 chars hex (formato persistido em `cred.json`).
fn hex_de(k: &[u8; 32]) -> String {
    k.iter().map(|b| format!("{b:02x}")).collect()
}

/// Converte uma chave hex de 64 chars (32 bytes) em `[u8;32]`. `None` se invalida.
fn parse_chave_hex(s: &str) -> Option<[u8; 32]> {
    let b = s.trim().as_bytes();
    if b.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        let hi = (b[i * 2] as char).to_digit(16)?;
        let lo = (b[i * 2 + 1] as char).to_digit(16)?;
        out[i] = (hi * 16 + lo) as u8;
    }
    Some(out)
}

/// Indice curto (0..255) estavel derivado do `tomada_id` (informativo no heartbeat).
fn short_id(tomada_id: &str) -> u8 {
    tomada_id.bytes().fold(0u8, |a, b| a.wrapping_add(b))
}
