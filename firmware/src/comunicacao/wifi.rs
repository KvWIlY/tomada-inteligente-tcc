//! Wi-Fi com NVS-first, retry, reconexao e Portal AP v2 (feature 005).
//!
//! Fluxo de boot:
//! 1. Le credenciais do NVS (chave `wifi_creds`).
//! 2. Se NVS vazio mas .env tem credenciais: usa .env e GRAVA no NVS.
//! 3. Se ambos vazios: vai pro Portal AP sem nem tentar STA.
//! 4. Tenta STA com retry+backoff (3 tentativas: 5s, 10s, 20s).
//! 5. Se TODAS falharem: abre o Portal AP — que, a partir da 005, CONTINUA tentando a
//!    rede salva em segundo plano e fecha sozinho quando ela volta (FR-002). Sem isso,
//!    uma queda de energia com roteador lento deixava a tomada presa no portal.
//!
//! Rede aberta (senha vazia) e suportada: `AuthMethod::None` (FR-016).
//! A causa de cada reabertura do portal e gravada em `motivo_portal` (uso unico) para
//! o form exibir o banner certo (FR-019/019a).

use anyhow::{anyhow, Result};
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    hal::modem::Modem,
    nvs::{EspDefaultNvsPartition, EspNvs},
    wifi::{AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi},
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use super::wifi_provisioning::{self, ContextoPortal, CredsRecebidas, SaidaPortal};
use crate::config_nuvem::{WIFI_PASSWORD, WIFI_SSID};
use tomada_solver::portal::MotivoPortal;

const NVS_NAMESPACE: &str = "wifi_creds";
const NVS_KEY_SSID: &str = "ssid";
const NVS_KEY_PWD: &str = "pwd";
/// BUILD_ID do firmware que rodou por ultimo (reset de dev a cada flash novo).
const NVS_KEY_BUILD: &str = "build_id";
/// Flag que pede o Portal AP no proximo boot.
const NVS_KEY_FORCAR: &str = "forcar_prov";
/// Causa da reabertura do portal (uso unico) — `MotivoPortal::as_str`.
const NVS_KEY_MOTIVO: &str = "motivo_portal";

const MAX_TENTATIVAS: u32 = 3;
const BACKOFF_SEGUNDOS: [u64; 3] = [5, 10, 20];
const INTERVALO_RECONEXAO_S: u64 = 30;

static CONECTADO: AtomicBool = AtomicBool::new(false);
static NVS_PART: OnceLock<EspDefaultNvsPartition> = OnceLock::new();

pub struct WifiHandle {
    _wifi: Arc<Mutex<BlockingWifi<EspWifi<'static>>>>,
}

/// O que o boot precisa saber depois que a rede foi resolvida.
pub enum ResultadoWifi {
    /// Conectado (ou operando sem rede, no caso standalone): segue o boot.
    Pronto(WifiHandle),
    /// O portal devolveu um formulario: o `main` aplica e reinicia.
    Configurar(Box<CredsRecebidas>),
}

#[allow(dead_code)]
pub fn esta_conectado() -> bool {
    CONECTADO.load(Ordering::Relaxed)
}

/// Config de cliente para uma rede JA salva (usada pelo portal no modo AP+STA).
/// A senha vem da NVS; senha vazia => rede aberta (FR-016).
pub(super) fn client_config_salvo(ssid: &str) -> Result<ClientConfiguration> {
    let pwd = NVS_PART
        .get()
        .and_then(|p| EspNvs::new(p.clone(), NVS_NAMESPACE, true).ok())
        .and_then(|nvs| {
            let mut buf = [0u8; 96];
            nvs.get_str(NVS_KEY_PWD, &mut buf)
                .ok()
                .flatten()
                .map(|s| s.to_string())
        })
        .unwrap_or_default();
    montar_client_config(ssid, &pwd)
}

/// Monta a config STA escolhendo o metodo de autenticacao pela senha (FR-016).
fn montar_client_config(ssid: &str, pwd: &str) -> Result<ClientConfiguration> {
    if ssid.len() > 32 {
        return Err(anyhow!("SSID excede 32 bytes"));
    }
    Ok(ClientConfiguration {
        ssid: ssid
            .try_into()
            .map_err(|_| anyhow!("SSID muito longo (max 32 bytes)"))?,
        password: pwd
            .try_into()
            .map_err(|_| anyhow!("password muito longa (max 63 bytes)"))?,
        // Rede aberta: sem isto o driver tenta WPA2 e a conexao NUNCA completa.
        auth_method: if pwd.is_empty() {
            AuthMethod::None
        } else {
            AuthMethod::WPA2Personal
        },
        ..Default::default()
    })
}

/// Ponto de entrada da rede. Pode devolver um formulario para o `main` aplicar.
pub fn conectar(modem: Modem<'static>, ctx: ContextoBoot) -> Result<ResultadoWifi> {
    let sysloop = EspSystemEventLoop::take()?;
    let nvs_part = EspDefaultNvsPartition::take()?;
    let _ = NVS_PART.set(nvs_part.clone());

    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(modem, sysloop.clone(), Some(nvs_part.clone()))?,
        sysloop,
    )?;

    let forcado = if eh_novo_build(&nvs_part) {
        log::warn!("[WIFI] BUILD NOVO -- credenciais limpas, abrindo portal AP");
        true
    } else if consumir_flag_forcar(&nvs_part) {
        log::warn!("[WIFI] portal FORCADO -- abrindo portal AP");
        true
    } else {
        false
    };

    if !forcado {
        if let Some(c) = resolver_credenciais(&nvs_part) {
            log::info!("[WIFI] credenciais disponiveis (SSID={}), tentando STA", c.ssid);
            match tentar_sta_com_retry(&mut wifi, &c) {
                Ok(()) => {
                    log_status_conectado(&wifi);
                    CONECTADO.store(true, Ordering::Relaxed);
                    return Ok(ResultadoWifi::Pronto(finalizar_handle_e_spawn_reconexao(
                        wifi,
                    )?));
                }
                Err(e) => {
                    log::warn!("[WIFI] STA esgotou tentativas: {e:?}");
                    // Causa provavel para o banner do portal. O radio nao separa com
                    // seguranca senha errada de rede fora de alcance -> mensagem
                    // combinada (FR-019).
                    gravar_motivo(&nvs_part, MotivoPortal::SenhaWifi);
                }
            }
        } else if ctx.modo_standalone_sem_rede {
            // Standalone sem Wi-Fi configurado: nao ha nada a conectar e o portal nao
            // deve abrir — a tomada opera offline (FR-006).
            log::warn!("[WIFI] standalone sem rede configurada -- operando sem Wi-Fi");
            return Ok(ResultadoWifi::Pronto(finalizar_handle_e_spawn_reconexao(
                wifi,
            )?));
        } else {
            log::warn!("[WIFI] nenhuma credencial encontrada (NVS e .env vazios)");
        }
    }

    // Portal AP v2: bloqueia ate receber o formulario OU ate a rede salva voltar.
    let ssid_salvo = resolver_credenciais(&nvs_part).map(|c| c.ssid);
    let portal_ctx = ContextoPortal {
        ssid_salvo,
        motivo: consumir_motivo(&nvs_part),
        servidor_default: crate::config_nuvem::backend_url(),
        tem_identidade_nuvem: ctx.tem_identidade_nuvem,
        pin_proprio: ctx.pin_proprio,
    };

    match wifi_provisioning::iniciar(&mut wifi, portal_ctx)? {
        SaidaPortal::Reconectado => {
            log_status_conectado(&wifi);
            CONECTADO.store(true, Ordering::Relaxed);
            Ok(ResultadoWifi::Pronto(finalizar_handle_e_spawn_reconexao(
                wifi,
            )?))
        }
        SaidaPortal::Configurado(creds) => Ok(ResultadoWifi::Configurar(creds)),
    }
}

/// Contexto que o `main` passa (o wifi nao le LittleFS sozinho).
pub struct ContextoBoot {
    pub tem_identidade_nuvem: bool,
    pub pin_proprio: String,
    pub modo_standalone_sem_rede: bool,
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
    // Senha AUSENTE e senha VAZIA sao coisas diferentes: vazia = rede aberta valida.
    let pwd_nvs = nvs
        .get_str(NVS_KEY_PWD, pwd_buf)
        .ok()
        .flatten()
        .unwrap_or("");

    if let Some(s) = ssid_nvs {
        if !s.is_empty() {
            log::info!("[WIFI] usando credenciais do NVS");
            return Some(Credenciais {
                ssid: s.to_string(),
                pwd: pwd_nvs.to_string(),
            });
        }
    }

    if !WIFI_SSID.is_empty() {
        log::info!("[WIFI] NVS vazio, usando .env (fallback de desenvolvimento)");
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

/// Grava so a senha, preservando o SSID salvo (FR-017: trocar a senha do mesmo AP).
pub fn salvar_senha_nvs(pwd: &str) -> Result<()> {
    let part = NVS_PART.get().ok_or_else(|| anyhow!("NVS nao inicializada"))?;
    let nvs = EspNvs::new(part.clone(), NVS_NAMESPACE, true)?;
    nvs.set_str(NVS_KEY_PWD, pwd)?;
    Ok(())
}

/// Grava SSID+senha (usado pelo `main` ao aplicar o formulario).
pub fn salvar_wifi(ssid: &str, pwd: &str) -> Result<()> {
    let part = NVS_PART.get().ok_or_else(|| anyhow!("NVS nao inicializada"))?;
    salvar_credenciais_nvs(part, ssid, pwd)
}

// =============================================
// Reset de dev / flags de portal
// =============================================

fn eh_novo_build(nvs_part: &EspDefaultNvsPartition) -> bool {
    let atual = env!("BUILD_ID");
    let nvs = match EspNvs::new(nvs_part.clone(), NVS_NAMESPACE, true) {
        Ok(n) => n,
        Err(_) => return false,
    };
    let mut buf = [0_u8; 40];
    let gravado = nvs
        .get_str(NVS_KEY_BUILD, &mut buf)
        .ok()
        .flatten()
        .unwrap_or("");
    if gravado == atual {
        return false;
    }
    log::warn!("[WIFI] build novo (id \"{gravado}\" -> \"{atual}\") -- resetando credenciais de dev");
    let _ = nvs.remove(NVS_KEY_SSID);
    let _ = nvs.remove(NVS_KEY_PWD);
    crate::persistencia::credenciais::limpar_tudo();
    crate::persistencia::medicao::limpar_tudo();
    if let Err(e) = nvs.set_str(NVS_KEY_BUILD, atual) {
        log::error!("[WIFI] falha gravando BUILD_ID no NVS: {e:?}");
    }
    true
}

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
        let _ = nvs.remove(NVS_KEY_FORCAR);
    }
    setada
}

fn gravar_motivo(nvs_part: &EspDefaultNvsPartition, motivo: MotivoPortal) {
    if let Ok(nvs) = EspNvs::new(nvs_part.clone(), NVS_NAMESPACE, true) {
        let _ = nvs.set_str(NVS_KEY_MOTIVO, motivo.as_str());
    }
}

/// Le e limpa a causa da reabertura (uso unico — FR-019).
fn consumir_motivo(nvs_part: &EspDefaultNvsPartition) -> Option<MotivoPortal> {
    let nvs = EspNvs::new(nvs_part.clone(), NVS_NAMESPACE, true).ok()?;
    let mut buf = [0u8; 32];
    let m = nvs
        .get_str(NVS_KEY_MOTIVO, &mut buf)
        .ok()
        .flatten()
        .and_then(MotivoPortal::parse);
    if m.is_some() {
        let _ = nvs.remove(NVS_KEY_MOTIVO);
    }
    m
}

/// Reabre o Portal AP no proximo boot, registrando a causa (FR-019).
/// Mantem o Wi-Fi salvo: o usuario so reconfirma o que falhou.
pub fn forcar_portal_e_reiniciar(motivo: MotivoPortal) -> ! {
    if let Some(part) = NVS_PART.get() {
        gravar_motivo(part, motivo);
    }
    marcar_portal_no_proximo_boot(false);
    log::warn!("[WIFI] reabrindo portal AP em 2s ({})...", motivo.as_str());
    std::thread::sleep(Duration::from_secs(2));
    unsafe { esp_idf_svc::sys::esp_restart() }
}

/// Reset de fabrica (UC25/UC27): apaga identidade (LittleFS) E Wi-Fi (NVS).
pub fn resetar_identidade_e_reabrir_portal() -> ! {
    crate::persistencia::credenciais::limpar_tudo();
    log::warn!("[WIFI] RESET: identidade (LittleFS) e Wi-Fi (NVS) apagados");
    marcar_portal_no_proximo_boot(true);
    std::thread::sleep(Duration::from_secs(2));
    unsafe { esp_idf_svc::sys::esp_restart() }
}

fn marcar_portal_no_proximo_boot(limpar_wifi: bool) {
    match NVS_PART.get() {
        Some(part) => match EspNvs::new(part.clone(), NVS_NAMESPACE, true) {
            Ok(nvs) => {
                if limpar_wifi {
                    let _ = nvs.remove(NVS_KEY_SSID);
                    let _ = nvs.remove(NVS_KEY_PWD);
                }
                if let Err(e) = nvs.set_str(NVS_KEY_FORCAR, "1") {
                    log::error!("[WIFI] falha gravando flag de portal: {e:?}");
                }
            }
            Err(e) => log::error!("[WIFI] NVS indisponivel p/ flag de portal: {e:?}"),
        },
        None => log::error!("[WIFI] particao NVS nao inicializada -- nao da p/ forcar portal"),
    }
}

// =============================================
// STA
// =============================================

fn tentar_sta_com_retry(
    wifi: &mut BlockingWifi<EspWifi<'static>>,
    creds: &Credenciais,
) -> Result<()> {
    wifi.set_configuration(&Configuration::Client(montar_client_config(
        &creds.ssid,
        &creds.pwd,
    )?))?;

    log::info!("[WIFI] start...");
    wifi.start()?;

    let mut ultimo_erro: Option<anyhow::Error> = None;
    for tentativa in 1..=MAX_TENTATIVAS {
        log::info!(
            "[WIFI] tentativa {tentativa}/{MAX_TENTATIVAS} -- conectando em \"{}\"...",
            creds.ssid
        );
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
            ip_info.ip,
            ip_info.subnet.gateway,
            ip_info.subnet.mask,
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

        let ainda_ok = guard
            .wifi()
            .sta_netif()
            .get_ip_info()
            .map(|info| !info.ip.is_unspecified())
            .unwrap_or(false);

        if ainda_ok {
            CONECTADO.store(true, Ordering::Relaxed);
            continue;
        }

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
