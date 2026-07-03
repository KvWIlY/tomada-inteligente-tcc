//! Wi-Fi com NVS-first, retry, reconexao e modo provisioning.
//!
//! Fluxo de boot:
//! 1. Le credenciais do NVS (chave `wifi_creds`).
//! 2. Se NVS vazio mas .env tem credenciais: usa .env e GRAVA no NVS.
//! 3. Se ambos vazios: vai pra modo provisioning (AP) sem nem tentar STA.
//! 4. Tenta STA com retry+backoff (3 tentativas: 5s, 10s, 20s).
//! 5. Se TODAS as tentativas falharem: entra em modo provisioning.
//!
//! Apos conectar com sucesso, spawn de uma task em background que
//! monitora a conexao a cada 30s e reconecta se cair.
//!
//! API:
//! - `conectar(modem)` -- ponto de entrada principal
//! - `esta_conectado()` -- status global pra outros modulos consultarem

use anyhow::{anyhow, Result};
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    hal::modem::Modem,
    nvs::{EspDefaultNvsPartition, EspNvs},
    wifi::{BlockingWifi, ClientConfiguration, Configuration, EspWifi},
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use crate::config_nuvem::{WIFI_PASSWORD, WIFI_SSID};
use super::wifi_provisioning;

const NVS_NAMESPACE: &str = "wifi_creds";
const NVS_KEY_SSID: &str = "ssid";
const NVS_KEY_PWD:  &str = "pwd";
/// BUILD_ID do firmware que rodou por ultimo (gravado no NVS). Difere do BUILD_ID
/// compilado => build novo => reset de dev (portal AP + reprovisionamento limpo).
const NVS_KEY_BUILD: &str = "build_id";
/// Flag (NVS) que pede o portal AP no proximo boot. Setada quando o provisionamento
/// falha por claim invalido (`forcar_portal_e_reiniciar`) — sem ela o no ficaria preso
/// retentando o mesmo claim ruim, com o portal ja fechado.
const NVS_KEY_FORCAR: &str = "forcar_prov";

const MAX_TENTATIVAS: u32 = 3;
const BACKOFF_SEGUNDOS: [u64; 3] = [5, 10, 20];
const INTERVALO_RECONEXAO_S: u64 = 30;

static CONECTADO: AtomicBool = AtomicBool::new(false);
/// Particao NVS guardada no boot p/ `forcar_portal_e_reiniciar` setar a flag depois.
static NVS_PART: OnceLock<EspDefaultNvsPartition> = OnceLock::new();

/// Mantem o radio Wi-Fi vivo e da acesso ao loop de reconexao via Mutex.
pub struct WifiHandle {
    _wifi: Arc<Mutex<BlockingWifi<EspWifi<'static>>>>,
}

/// Status global da conexao (atualizado pela task de reconexao em background).
#[allow(dead_code)] // disponivel pra outros modulos consultarem
pub fn esta_conectado() -> bool {
    CONECTADO.load(Ordering::Relaxed)
}

/// Ponto de entrada: conecta o ESP32 a uma rede Wi-Fi.
/// Pode entrar em modo provisioning (AP) e CHAMAR esp_restart() se necessario --
/// nesse caso esta funcao nao retorna (o programa reinicia do zero).
pub fn conectar(modem: Modem<'static>) -> Result<WifiHandle> {
    let sysloop = EspSystemEventLoop::take()?;
    let nvs_part = EspDefaultNvsPartition::take()?;
    // Guarda a particao p/ `forcar_portal_e_reiniciar` setar a flag depois (best-effort).
    let _ = NVS_PART.set(nvs_part.clone());

    // Cria a Wi-Fi UMA VEZ (modem so pode ser consumido uma vez)
    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(modem, sysloop.clone(), Some(nvs_part.clone()))?,
        sysloop,
    )?;

    // Decide STA x portal AP. Tem prioridade (abre o portal direto, ignorando credenciais):
    //   - build novo => reset de dev (limpa tudo);
    //   - flag forcar_prov => claim invalido no boot anterior (mantem o Wi-Fi salvo).
    if eh_novo_build(&nvs_part) {
        log::warn!("[WIFI] BUILD NOVO -- credenciais limpas, abrindo portal AP");
    } else if consumir_flag_forcar(&nvs_part) {
        log::warn!("[WIFI] portal FORCADO (claim invalido) -- abrindo portal AP p/ novo codigo");
    } else if let Some(c) = resolver_credenciais(&nvs_part) {
        log::info!("[WIFI] credenciais disponiveis (SSID={}), tentando STA", c.ssid);
        match tentar_sta_com_retry(&mut wifi, &c) {
            Ok(()) => {
                log_status_conectado(&wifi);
                CONECTADO.store(true, Ordering::Relaxed);
                return finalizar_handle_e_spawn_reconexao(wifi);
            }
            Err(e) => {
                log::warn!("[WIFI] STA esgotou tentativas: {e:?}");
                log::warn!("[WIFI] entrando em modo provisioning -- credenciais salvas talvez mudaram");
            }
        }
    } else {
        log::warn!("[WIFI] nenhuma credencial encontrada (NVS e .env vazios)");
        log::warn!("[WIFI] entrando em modo provisioning");
    }

    // Modo provisioning: bloqueia ate receber credenciais via form HTTP
    let novas = wifi_provisioning::iniciar(&mut wifi)?;

    // Salva no NVS e reinicia (proxima boot vai usar as credenciais novas)
    if let Err(e) = salvar_credenciais_nvs(&nvs_part, &novas.ssid, &novas.pwd) {
        log::error!("[WIFI] falha salvando NVS: {e:?}");
    }
    // Persiste o claim-code do portal em LittleFS p/ o provisionamento pos-restart.
    if let Err(e) = crate::persistencia::credenciais::salvar_claim_pendente(&novas.claim_code) {
        log::error!("[WIFI] falha salvando claim-code: {e:?}");
    }
    // Folga antes do restart: o AP (e o HTTP server) so caem aqui. 4s dao tempo
    // do celular renderizar a pagina "Configuracao salva" antes do AP sumir.
    log::info!("[WIFI] credenciais salvas. Reiniciando ESP32 em 4s...");
    std::thread::sleep(Duration::from_secs(4));
    unsafe { esp_idf_svc::sys::esp_restart(); }
    // unreachable -- esp_restart nao retorna
}

// =============================================
// Credenciais (NVS / .env)
// =============================================

pub(super) struct Credenciais {
    pub ssid: String,
    pub pwd: String,
}

fn resolver_credenciais(nvs_part: &EspDefaultNvsPartition) -> Option<Credenciais> {
    let nvs = EspNvs::new(nvs_part.clone(), NVS_NAMESPACE, true).ok()?;

    let ssid_buf = &mut [0_u8; 64];
    let pwd_buf = &mut [0_u8; 96];
    let ssid_nvs = nvs.get_str(NVS_KEY_SSID, ssid_buf).ok().flatten();
    let pwd_nvs = nvs.get_str(NVS_KEY_PWD, pwd_buf).ok().flatten();

    if let (Some(s), Some(p)) = (ssid_nvs, pwd_nvs) {
        if !s.is_empty() {
            log::info!("[WIFI] usando credenciais do NVS");
            return Some(Credenciais { ssid: s.to_string(), pwd: p.to_string() });
        }
    }

    // Fallback do .env (build-time) -- modo dev
    if !WIFI_SSID.is_empty() {
        log::info!("[WIFI] NVS vazio, usando .env (fallback de desenvolvimento)");
        // Grava no NVS pra proxima boot (a partir daqui, mesmo sem .env, fica funcionando)
        let _ = nvs.set_str(NVS_KEY_SSID, WIFI_SSID);
        let _ = nvs.set_str(NVS_KEY_PWD, WIFI_PASSWORD);
        return Some(Credenciais {
            ssid: WIFI_SSID.to_string(),
            pwd: WIFI_PASSWORD.to_string(),
        });
    }

    None
}

pub(super) fn salvar_credenciais_nvs(
    nvs_part: &EspDefaultNvsPartition,
    ssid: &str,
    pwd: &str,
) -> Result<()> {
    let nvs = EspNvs::new(nvs_part.clone(), NVS_NAMESPACE, true)?;
    nvs.set_str(NVS_KEY_SSID, ssid)?;
    nvs.set_str(NVS_KEY_PWD, pwd)?;
    Ok(())
}

// =============================================
// Reset de dev por build novo (substitui o botao de provisionamento)
// =============================================

/// Detecta build novo comparando o `BUILD_ID` compilado (injetado pelo `build.rs`) com o
/// gravado no NVS. Em build novo limpa o Wi-Fi salvo (NVS) e a identidade provisionada
/// (LittleFS: cred.json + claim), persiste o novo id e retorna `true` => o boot abre o
/// portal AP e o device reprovisiona do zero. Conveniencia de desenvolvimento: a cada
/// flash novo a tomada volta ao estado de fabrica, sem botao e sem apagar a flash inteira.
fn eh_novo_build(nvs_part: &EspDefaultNvsPartition) -> bool {
    let atual = env!("BUILD_ID");
    let nvs = match EspNvs::new(nvs_part.clone(), NVS_NAMESPACE, true) {
        Ok(n) => n,
        Err(_) => return false,
    };
    let mut buf = [0_u8; 40];
    let gravado = nvs.get_str(NVS_KEY_BUILD, &mut buf).ok().flatten().unwrap_or("");
    if gravado == atual {
        return false;
    }
    log::warn!("[WIFI] build novo (id \"{gravado}\" -> \"{atual}\") -- resetando credenciais de dev");
    let _ = nvs.remove(NVS_KEY_SSID);
    let _ = nvs.remove(NVS_KEY_PWD);
    crate::persistencia::credenciais::limpar_tudo();
    // Identidade nova => medicoes do tomada_id antigo dariam 403 na RLS e travariam o sync.
    crate::persistencia::medicao::limpar_tudo();
    if let Err(e) = nvs.set_str(NVS_KEY_BUILD, atual) {
        log::error!("[WIFI] falha gravando BUILD_ID no NVS: {e:?}");
    }
    true
}

/// Le e limpa (uso unico) a flag de portal forcado. `true` => abrir o portal AP neste boot.
fn consumir_flag_forcar(nvs_part: &EspDefaultNvsPartition) -> bool {
    let nvs = match EspNvs::new(nvs_part.clone(), NVS_NAMESPACE, true) {
        Ok(n) => n,
        Err(_) => return false,
    };
    let mut buf = [0_u8; 8];
    let setada = nvs
        .get_str(NVS_KEY_FORCAR, &mut buf)
        .ok()
        .flatten()
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    if setada {
        let _ = nvs.remove(NVS_KEY_FORCAR); // consome: nao reabre no boot seguinte
    }
    setada
}

/// Marca a flag de portal e reinicia o chip. Chamado pelo loop de provisionamento quando
/// o claim e invalido: o portal AP reabre no proximo boot para coletar um novo claim
/// (o Wi-Fi salvo e mantido — o usuario so reconfirma a rede no form). Nao retorna.
pub fn forcar_portal_e_reiniciar() -> ! {
    match NVS_PART.get() {
        Some(part) => match EspNvs::new(part.clone(), NVS_NAMESPACE, true) {
            Ok(nvs) => {
                if let Err(e) = nvs.set_str(NVS_KEY_FORCAR, "1") {
                    log::error!("[WIFI] falha gravando flag de portal: {e:?}");
                }
            }
            Err(e) => log::error!("[WIFI] NVS indisponivel p/ flag de portal: {e:?}"),
        },
        None => log::error!("[WIFI] particao NVS nao inicializada -- nao da p/ forcar portal"),
    }
    log::warn!("[WIFI] reabrindo portal AP em 2s para novo claim-code...");
    std::thread::sleep(Duration::from_secs(2));
    unsafe { esp_idf_svc::sys::esp_restart() } // -> ! (nao retorna)
}

// =============================================
// STA mode (cliente Wi-Fi)
// =============================================

fn tentar_sta_com_retry(
    wifi: &mut BlockingWifi<EspWifi<'static>>,
    creds: &Credenciais,
) -> Result<()> {
    let cfg_ssid = creds.ssid.as_str().try_into()
        .map_err(|_| anyhow!("SSID muito longo (max 32 chars)"))?;
    let cfg_pwd = creds.pwd.as_str().try_into()
        .map_err(|_| anyhow!("password muito longa (max 64 chars)"))?;

    wifi.set_configuration(&Configuration::Client(ClientConfiguration {
        ssid: cfg_ssid,
        password: cfg_pwd,
        ..Default::default()
    }))?;

    log::info!("[WIFI] start...");
    wifi.start()?;

    let mut ultimo_erro: Option<anyhow::Error> = None;
    for tentativa in 1..=MAX_TENTATIVAS {
        log::info!("[WIFI] tentativa {tentativa}/{MAX_TENTATIVAS} -- conectando em \"{}\"...", creds.ssid);
        match try_one_connect(wifi) {
            Ok(()) => return Ok(()),
            Err(e) => {
                log::warn!("[WIFI] tentativa {tentativa} falhou: {e:?}");
                ultimo_erro = Some(e);
                if tentativa < MAX_TENTATIVAS {
                    let pausa = BACKOFF_SEGUNDOS[(tentativa - 1) as usize];
                    log::info!("[WIFI] pausando {pausa}s antes da proxima tentativa");
                    std::thread::sleep(Duration::from_secs(pausa));
                    let _ = wifi.disconnect();
                }
            }
        }
    }
    Err(ultimo_erro.unwrap_or_else(|| anyhow!("Wi-Fi falhou todas as tentativas")))
}

fn try_one_connect(wifi: &mut BlockingWifi<EspWifi<'static>>) -> Result<()> {
    wifi.connect()?;
    wifi.wait_netif_up()?;
    Ok(())
}

fn log_status_conectado(wifi: &BlockingWifi<EspWifi<'static>>) {
    if let Ok(ip_info) = wifi.wifi().sta_netif().get_ip_info() {
        log::info!(
            "[WIFI] conectado -- IP={} gw={} mask={}",
            ip_info.ip, ip_info.subnet.gateway, ip_info.subnet.mask,
        );
    }
}

fn finalizar_handle_e_spawn_reconexao(
    wifi: BlockingWifi<EspWifi<'static>>,
) -> Result<WifiHandle> {
    let wifi_arc = Arc::new(Mutex::new(wifi));
    let wifi_clone = wifi_arc.clone();
    std::thread::Builder::new()
        .name("wifi-reconnect".into())
        .stack_size(4096)
        .spawn(move || loop_reconexao(wifi_clone))?;
    Ok(WifiHandle { _wifi: wifi_arc })
}

// =============================================
// Reconexao em background
// =============================================

fn loop_reconexao(wifi: Arc<Mutex<BlockingWifi<EspWifi<'static>>>>) {
    log::info!("[WIFI] task de reconexao iniciada (check a cada {INTERVALO_RECONEXAO_S}s)");
    loop {
        std::thread::sleep(Duration::from_secs(INTERVALO_RECONEXAO_S));

        let mut guard = match wifi.lock() {
            Ok(g) => g,
            Err(_) => {
                log::warn!("[WIFI] mutex envenenado, encerrando reconexao");
                return;
            }
        };

        // Esta conectado? Olha se tem IP valido.
        let ainda_ok = guard.wifi().sta_netif().get_ip_info()
            .map(|info| !info.ip.is_unspecified())
            .unwrap_or(false);

        if ainda_ok {
            CONECTADO.store(true, Ordering::Relaxed);
            continue;
        }

        // Caiu -> tenta voltar
        CONECTADO.store(false, Ordering::Relaxed);
        log::warn!("[WIFI] conexao caiu, tentando reconectar...");
        if let Err(e) = guard.connect() {
            log::warn!("[WIFI] reconnect: connect() falhou: {e:?}");
            continue;
        }
        if let Err(e) = guard.wait_netif_up() {
            log::warn!("[WIFI] reconnect: wait_netif_up falhou: {e:?}");
            continue;
        }
        log_status_conectado(&guard);
        CONECTADO.store(true, Ordering::Relaxed);
    }
}
