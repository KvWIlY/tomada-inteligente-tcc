//! Tomada inteligente — orquestracao do ciclo medir -> decidir -> atuar -> sincronizar.
//!
//! Boot: LittleFS -> Wi-Fi -> SNTP -> (provisionamento por-device se necessario) ->
//! ADC/rele/ESP-NOW. Loop (cadencia 10 s, SC-008):
//!   medir (RMS V/I) -> trocar heartbeats ESP-NOW -> decidir (MILP) -> atuar rele.
//! Sem NENHUM peer ESP-NOW vivo e online (003): irmas lidas da nuvem no cloud tick
//! viram carga fixa contra o P_max (cache RAM; zera em falha — local-only).
//! A cada ~60 s (cloud tick): bufferiza medicao (1/min), faz pull do desired state
//! (config por versao + comando manual de rele), publica telemetria/estado/calibracao,
//! sincroniza medicoes+eventos EM LOTE (Bearer JWT por-device) e roda a retencao.
//!
//! Tudo de nuvem e best-effort: sem conectividade o no decide e atua localmente
//! (offline-first, Principio VI) e reconcilia ao reconectar.

use anyhow::Result;
use chrono::{Local, Timelike};
use esp_idf_svc::hal::adc::attenuation::DB_12;
use esp_idf_svc::hal::adc::oneshot::config::AdcChannelConfig;
use esp_idf_svc::hal::adc::oneshot::{AdcChannelDriver, AdcDriver};
use esp_idf_svc::hal::peripherals::Peripherals;
use std::thread;
use std::time::Duration;

mod comunicacao;
mod config_nuvem;
mod decisao;
mod persistencia;
mod sensores;

use comunicacao::espnow::{self, Heartbeat};
use comunicacao::{provisionamento, supabase};
use decisao::atuador::Atuador;
use decisao::ciclo::Ciclo;
use persistencia::config;
use persistencia::credenciais::{self, CredenciaisLocais};
use persistencia::medicao::{self, Medicao};
use persistencia::retencao;
use tomada_solver::coordenacao::{carga_fixa_nuvem, contar_frescas, RemoteTomada};
use tomada_solver::sync::deve_adotar_config;

/// Cadencia do ciclo de decisao (s) — SC-008 (decisao em <=10 s).
const CICLO_S: u64 = 10;
/// A cada quantos ciclos fazemos I/O de nuvem (~60 s => medicao 1/min).
const CICLOS_POR_NUVEM: u64 = 6;
/// TTL de um peer ESP-NOW (vivo se visto nesse intervalo) — 3x a cadencia.
const TTL_PEER_S: i64 = (CICLO_S as i64) * 3;
/// Janela de frescor do fallback de coordenacao pela nuvem (003): irma contada
/// sse `agora - ultimo_uso_ativo_ts <= janela` (~5 cloud ticks de folga).
const JANELA_FRESCOR_S: i64 = 300;

fn main() -> Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("=== tomada inteligente: boot ===");

    // 1. LittleFS
    persistencia::init()?;

    // 2. Wi-Fi (STA; cai para portal AP se sem credenciais)
    let peripherals = Peripherals::take()?;
    let _wifi = comunicacao::wifi::conectar(peripherals.modem)?;

    // 3. SNTP (relogio real para timestamps/idempotencia)
    let _sntp = comunicacao::sntp::sincronizar()?;

    // 4. Identidade/credenciais por-device (provisiona no 1o boot). Tenta ate conseguir
    //    (modo auto: sem claim, o servidor registra numa residencia padrao).
    let creds = loop {
        match obter_ou_provisionar()? {
            Provisao::Pronto(c) => break c,
            // Claim ruim: retry e inutil. Reabre o portal AP (nao retorna -> reinicia).
            Provisao::ReabrirPortal => comunicacao::wifi::forcar_portal_e_reiniciar(),
            Provisao::Retentar => {
                log::warn!("[PROV] nova tentativa de provisionamento em 15s...");
                thread::sleep(Duration::from_secs(15));
            }
        }
    };
    log::info!("[BOOT] tomada_id={}", creds.tomada_id);

    // 5. ADC (oneshot): GPIO32 = tensao (ZMPT101B), GPIO35 = corrente (SCT-013).
    //    NOTA: a API oneshot (AdcChannelConfig/read) pode variar de versao; ponto provavel
    //    de ajuste fino ao compilar. `read()` devolve mV calibrados (calibration=true).
    let adc1 = AdcDriver::new(peripherals.adc1)?;
    // calibration=None (default): `read()` ainda devolve mV via converter direto, sem
    // depender de esquema de calibracao por hardware (robusto no ESP32 classico).
    let ch_cfg = AdcChannelConfig {
        attenuation: DB_12, // ~0..3.1 V (rede atenuada cabe na faixa)
        ..Default::default()
    };
    let mut ch_v = AdcChannelDriver::new(&adc1, peripherals.pins.gpio32, &ch_cfg)?;
    let mut ch_i = AdcChannelDriver::new(&adc1, peripherals.pins.gpio35, &ch_cfg)?;

    // 6. Rele (GPIO26, Opcao A). Comeca aberto (OFF).
    let mut atuador = Atuador::novo(peripherals.pins.gpio26)?;

    // 7. ESP-NOW (best-effort; exige a residencia_key da casa p/ autenticar — Principio
    //    VIII). Sem chave valida (credencial antiga), a coordenacao fica DESABILITADA
    //    (fail-closed) e o no opera sozinho ate re-provisionar.
    let espnow = match parse_chave_hex(&creds.residencia_key) {
        Some(key) => match espnow::iniciar(key) {
            Ok(e) => Some(e),
            Err(e) => {
                log::warn!("[BOOT] ESP-NOW indisponivel ({e}); operando sozinho");
                None
            }
        },
        None => {
            log::warn!(
                "[BOOT] sem residencia_key valida -> ESP-NOW DESABILITADO (re-provisione p/ \
                 coordenar). Operando sozinho."
            );
            None
        }
    };

    // 8. Estado do ciclo
    let mut config = config::load()?;
    let agora0 = provisionamento::agora_epoch();
    let mut ciclo = Ciclo::novo(&config, 0.0, 0.0, agora0);
    let short_id = short_id(&creds.tomada_id);
    // MAC proprio: chave de ordenacao canonica da coordenacao ESP-NOW (decide a posicao
    // deste no na lista resolvida pelo MILP, identica em todos os nos).
    let proprio_mac = provisionamento::obter_mac_bytes().unwrap_or([0u8; 6]);
    let mut rele_manual: Option<bool> = None;
    // Cache RAM do fallback de coordenacao pela nuvem (003): recalculado no cloud
    // tick, aplicado nos ciclos intermediarios, zerado em falha/quando ha peer vivo.
    let mut carga_nuvem: f32 = 0.0;

    log::info!("[BOOT] entrando no loop principal (ciclo={CICLO_S}s)");

    let mut n: u64 = 0;
    loop {
        let agora = provisionamento::agora_epoch();
        let hora = Local::now().hour() as u8;
        let h = (hora % 24) as usize;
        let cloud_tick = n % CICLOS_POR_NUVEM == 0;

        // --- MEDIR (RMS de V e I; ADC isolado em closures) ---
        let (leitura, diag) = sensores::medir(
            || ch_v.read().unwrap_or(0) as f32,
            || ch_i.read().unwrap_or(0) as f32,
        );
        // DIAGNOSTICO (temporario): RMS bruto em mV antes da calibracao, p/ caracterizar
        // o piso de ruido no console (nada plugado) e cravar o limiar do gate. Remover
        // depois de calibrar. v_ac/i_ac = componente AC; v_dc/i_dc = bias DC do canal.
        log::info!(
            "[DIAG] V_dc={:.0}mV V_ac={:.1}mV | I_dc={:.0}mV I_ac={:.2}mV",
            diag.v_dc_mv, diag.v_ac_mv, diag.i_dc_mv, diag.i_ac_mv,
        );

        // --- COORDENACAO ESP-NOW (peers vivos + carga fixa dos ausentes) ---
        let (peers_vivos, carga_fixa) = espnow
            .as_ref()
            .map(|_| espnow::montar_coordenacao(agora, TTL_PEER_S))
            .unwrap_or((Vec::new(), 0.0));

        // --- FALLBACK DE COORDENACAO PELA NUVEM (003) ---
        // So quando o mesh nao trouxe NENHUM peer vivo: fontes nunca se misturam
        // (sem correlacao MAC<->UUID confiavel => dedup impossivel; ver research D2).
        if !peers_vivos.is_empty() {
            carga_nuvem = 0.0; // mesh tem prioridade; invalida o cache
        } else if cloud_tick {
            carga_nuvem = if comunicacao::wifi::esta_conectado() {
                match supabase::buscar_todas_tomadas() {
                    Ok(remotas) => {
                        let irmas: Vec<RemoteTomada> = remotas
                            .into_iter()
                            .map(|t| RemoteTomada {
                                tomada_id: t.id,
                                p_medida_atual: t.p_medida_atual,
                                ultimo_uso_ativo_ts: t.ultimo_uso_ativo_ts,
                            })
                            .collect();
                        let carga = carga_fixa_nuvem(
                            &creds.tomada_id, agora, JANELA_FRESCOR_S, &irmas,
                        );
                        if carga > 0.0 {
                            let frescas =
                                contar_frescas(&creds.tomada_id, agora, JANELA_FRESCOR_S, &irmas);
                            let total =
                                irmas.iter().filter(|t| t.tomada_id != creds.tomada_id).count();
                            supabase::registrar_evento(
                                "coordenacao_nuvem",
                                "info",
                                "Fallback de coordenacao pela nuvem",
                                &format!("carga_fixa={carga:.1}W irmas_frescas={frescas}/{total}"),
                            );
                        }
                        carga
                    }
                    Err(e) => {
                        // Sem dado confiavel neste periodo -> local-only (research D3).
                        log::warn!("[NUVEM] fallback coordenacao: GET falhou ({e}); carga=0");
                        0.0
                    }
                }
            } else {
                0.0 // offline: comportamento local-only ja validado
            };
        }
        let carga_fixa = carga_fixa + carga_nuvem;

        // --- DECIDIR (MILP) ---
        let decisao = ciclo.decidir(
            &config,
            leitura,
            &peers_vivos,
            carga_fixa,
            proprio_mac,
            rele_manual,
            hora,
            agora,
        );

        // --- ATUAR ---
        if let Err(e) = atuador.aplicar(decisao.estado) {
            log::error!("[CICLO] falha ao atuar rele: {e:?}");
        }
        log::info!(
            "[CICLO] V={:.1} I={:.2} P={:.1}W estado={} rele_aberto={} peers={} carga_fixa={:.0}",
            leitura.vrms, leitura.irms, leitura.p, decisao.estado.as_str(),
            decisao.rele_aberto, peers_vivos.len(), carga_fixa,
        );

        // --- DIFUNDIR HEARTBEAT ---
        if let Some(en) = &espnow {
            let hb = Heartbeat::novo(
                short_id,
                agora as u32,
                decisao.p_medida,
                decisao.estado.idx() as u8,
                decisao.u_i,
                decisao.f_i,
                rele_manual.unwrap_or(false),
                config.schedule[h],
                decisao.cstd,
                decisao.con,
                config.pi_historico[h],
                config.w_efetivo(),
            );
            let _ = espnow::difundir(en, &hb);
        }

        // --- Eventos do ciclo -> fila (enviados no cloud tick) ---
        for e in &decisao.eventos {
            supabase::registrar_evento(e.tipo, e.severidade, &e.titulo, &e.detalhes);
        }

        // --- NUVEM (a cada ~60 s) ---
        if cloud_tick {
            // medicao 1/min (buffer em RAM; flush/sync abaixo)
            medicao::registrar(Medicao::novo(
                &creds.tomada_id,
                agora,
                decisao.p_medida,
                decisao.estado,
                decisao.rele_aberto,
            ));

            // pull do desired state (config por versao + comando manual)
            match supabase::buscar_desired_state() {
                Ok(ds) => {
                    rele_manual = ds.rele_aberto;
                    if let Some(remota) = ds.config {
                        if deve_adotar_config(remota.config_version, config.config_version) {
                            if let Err(e) = config::save(&remota) {
                                log::warn!("[NUVEM] falha salvando config remota: {e:?}");
                            } else {
                                log::info!(
                                    "[NUVEM] config adotada v{} -> v{}",
                                    config.config_version, remota.config_version
                                );
                                config = remota;
                            }
                        }
                    }
                }
                Err(e) => log::warn!("[NUVEM] pull desired_state falhou: {e:?}"),
            }

            // P_max canonico vem da RESIDENCIA (fonte de verdade, editavel no dashboard),
            // nao do jsonb da tomada. Aplica por cima da config (mesmo recem-adotada) e
            // persiste so quando muda (offline-first; evita desgaste da flash).
            match supabase::buscar_p_max_residencia() {
                Ok(Some(pmax)) if (pmax - config.p_max).abs() > 0.5 => {
                    log::info!("[NUVEM] P_max da residencia: {:.0} -> {:.0} W", config.p_max, pmax);
                    config.p_max = pmax;
                    if let Err(e) = config::save(&config) {
                        log::warn!("[NUVEM] falha persistindo P_max: {e:?}");
                    }
                }
                Ok(_) => {}
                Err(e) => log::warn!("[NUVEM] pull P_max falhou: {e:?}"),
            }

            // publica telemetria/estado/calibracao (best-effort)
            let _ = supabase::atualizar_telemetria(decisao.p_medida, decisao.ultimo_uso_ativo_ts);
            let _ = supabase::atualizar_estado(decisao.estado.as_str());
            if decisao.cstd > 0.0 || decisao.con > 0.0 {
                let _ = supabase::atualizar_calibracao(decisao.cstd, decisao.con);
            }

            // sync EM LOTE (Bearer JWT) + eventos
            match supabase::sincronizar_medicoes() {
                Ok(k) if k > 0 => log::info!("[NUVEM] {k} medicoes sincronizadas"),
                Ok(_) => {}
                Err(e) => log::warn!("[NUVEM] sync medicoes falhou: {e:?}"),
            }
            if let Err(e) = supabase::sincronizar_eventos() {
                log::warn!("[NUVEM] sync eventos falhou: {e:?}");
            }

            // retencao/purga (roda de fato a cada 5 dias)
            if let Err(e) = retencao::talvez_purgar(agora) {
                log::warn!("[NUVEM] retencao falhou: {e:?}");
            }

            // limpeza de peers muito antigos
            espnow::esquecer_antigos(agora, TTL_PEER_S * 10);
        }

        n = n.wrapping_add(1);
        thread::sleep(Duration::from_secs(CICLO_S));
    }
}

/// Resultado de uma tentativa de provisionamento (decidido pelo caller no loop de boot).
enum Provisao {
    /// Credenciais prontas (ja existiam ou provisionou agora).
    Pronto(CredenciaisLocais),
    /// Claim invalido/expirado/usado: reabrir o portal AP p/ um novo codigo.
    ReabrirPortal,
    /// Falha transitoria (rede/servidor): tentar de novo depois.
    Retentar,
}

/// Carrega credenciais; se ausentes, provisiona. Prioridade do claim-code:
///   1) portal AP (LittleFS)  2) `.env` (CLAIM_CODE)  3) VAZIO => modo AUTO (sem login).
/// No modo auto o servidor registra o MAC numa residencia padrao (demo single-tenant).
fn obter_ou_provisionar() -> Result<Provisao> {
    if let Some(c) = credenciais::load()? {
        return Ok(Provisao::Pronto(c));
    }
    let mac = provisionamento::obter_mac()?;

    let portal = credenciais::ler_claim_pendente();
    let claim = portal
        .as_deref()
        .filter(|c| !c.is_empty())
        .or_else(|| {
            let c = config_nuvem::CLAIM_CODE.trim();
            if c.is_empty() { None } else { Some(c) }
        })
        .unwrap_or("");

    log::info!(
        "[PROV] provisionando MAC={mac} (modo={})...",
        if claim.is_empty() { "AUTO (sem claim)" } else { "claim-code" }
    );
    match provisionamento::provisionar(&mac, claim) {
        Ok(creds) => {
            credenciais::limpar_claim_pendente(); // uso unico (se veio do portal)
            Ok(Provisao::Pronto(creds))
        }
        Err(provisionamento::ErroProv::ClaimInvalido) => {
            log::error!("[PROV] claim invalido/expirado/usado -- descartando e reabrindo portal");
            credenciais::limpar_claim_pendente(); // nao reutiliza o claim ruim
            Ok(Provisao::ReabrirPortal)
        }
        Err(provisionamento::ErroProv::Transitorio(e)) => {
            log::warn!("[PROV] provisionamento falhou: {e}");
            Ok(Provisao::Retentar)
        }
    }
}

/// Converte uma chave hex de 64 chars (32 bytes) em `[u8;32]`. `None` se invalida.
/// Byte-safe (opera em `as_bytes`, nao fatia por char — evita panico com UTF-8).
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

/// Indice curto (0..255) estavel derivado do `tomada_id` UUID (informativo no heartbeat;
/// a tabela de peers e chaveada pelo MAC do remetente, sem depender deste id).
fn short_id(tomada_id: &str) -> u8 {
    tomada_id.bytes().fold(0u8, |a, b| a.wrapping_add(b))
}
