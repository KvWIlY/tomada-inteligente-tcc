//! Portal AP v2 — configuracao local da tomada (feature 005, `contracts/portal-ap.md`).
//!
//! Diferencas para o v1 (todas motivadas por falhas reais de campo):
//!   - **AP+STA**: enquanto o portal esta aberto, o no continua tentando a rede salva a
//!     cada 30 s. Sem isso, uma queda de energia em que o roteador volta depois da
//!     tomada deixava o no preso no portal PARA SEMPRE (FR-002).
//!   - **Corpo por `Content-Length`**: o v1 fazia um unico `read` de 512 B e gravava
//!     senha truncada em silencio (FR-015).
//!   - **Modos**: nuvem | standalone | parear — a tomada deixa de exigir servidor e
//!     claim-code para funcionar (FR-004/008).
//!   - **Banner de causa** e **preservacao do Wi-Fi salvo** (FR-017/019).
//!
//! A validacao vive em `crate::solver::portal` (pura, testada no host); aqui so ha I/O.

use anyhow::{anyhow, Result};
use embedded_svc::io::Write;
use esp_idf_svc::{
    http::{
        server::{Configuration as HttpServerConfig, EspHttpServer},
        Method,
    },
    ipv4::{Configuration as IpConfiguration, Ipv4Addr, Mask, RouterConfiguration, Subnet},
    netif::{EspNetif, NetifConfiguration},
    wifi::{
        AccessPointConfiguration, AuthMethod, BlockingWifi, ClientConfiguration, Configuration,
        EspWifi,
    },
};
use std::net::UdpSocket;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::solver::portal::{
    self, ErroForm, FormPortal, Modo, MotivoPortal, ResolucaoWifi, MAX_SSID_BYTES,
};

const AP_PASSWORD: &str = "tomadatcc";
const AP_CHANNEL: u8 = 1;
const AP_MAX_CLIENTS: u16 = 4;
/// IP do AP do portal. E o mesmo default do esp-idf-svc (`RouterConfiguration`), so
/// explicito aqui porque o DNS do captive portal precisa apontar para ele.
const AP_IP: Ipv4Addr = Ipv4Addr::new(192, 168, 71, 1);
/// Intervalo entre tentativas de reconexao STA com o portal aberto (FR-002).
/// 30 s + associacao/DHCP (~20 s no pior caso) cabe folgado nos 2 min do SC-001.
const INTERVALO_RETRY_STA: Duration = Duration::from_secs(30);

/// O que o portal devolve ao fluxo de boot.
pub enum SaidaPortal {
    /// O usuario submeteu o formulario.
    Configurado(Box<CredsRecebidas>),
    /// A rede salva voltou e o STA reconectou sozinho: segue o boot SEM reiniciar
    /// e sem intervencao (FR-002).
    Reconectado,
}

/// Formulario aceito, ja validado e com a decisao de Wi-Fi resolvida.
pub struct CredsRecebidas {
    pub form: FormPortal,
    /// O que fazer com a NVS de Wi-Fi (preservar / gravar / trocar so a senha).
    pub wifi: ResolucaoWifi,
}

/// Contexto que o boot passa ao portal (o portal nao le NVS nem LittleFS sozinho).
pub struct ContextoPortal {
    pub ssid_salvo: Option<String>,
    /// Causa da reabertura, ja consumida da NVS pelo chamador (uso unico).
    pub motivo: Option<MotivoPortal>,
    /// Valor pre-preenchido do campo "servidor" (NVS -> build).
    pub servidor_default: String,
    /// Se ja existe identidade de nuvem (exige confirmacao para virar standalone).
    pub tem_identidade_nuvem: bool,
    /// PIN DESTA tomada — exibido no portal para quem for parear a partir dela.
    pub pin_proprio: String,
}

pub fn iniciar(
    wifi: &mut BlockingWifi<EspWifi<'static>>,
    ctx: ContextoPortal,
) -> Result<SaidaPortal> {
    let redes = match fazer_scan(wifi) {
        Ok(r) => {
            log::info!("[PROV] {} redes encontradas no scan", r.len());
            r
        }
        Err(e) => {
            log::warn!("[PROV] scan falhou: {e:?}, seguindo sem lista pre-pronta");
            vec![]
        }
    };

    let ap_ssid_str = montar_ap_ssid(wifi);
    configurar_ap_e_sta(wifi, &ap_ssid_str, ctx.ssid_salvo.as_deref())?;
    wifi.start()?;

    let ip_octets = wifi
        .wifi()
        .ap_netif()
        .get_ip_info()
        .map(|i| i.ip.octets())
        .unwrap_or(AP_IP.octets());
    let ip_ap = format!(
        "{}.{}.{}.{}",
        ip_octets[0], ip_octets[1], ip_octets[2], ip_octets[3]
    );

    match iniciar_dns_captive(ip_octets) {
        Ok(()) => log::info!("[PROV] captive portal DNS ativo (-> {ip_ap})"),
        Err(e) => log::warn!("[PROV] DNS captive falhou ({e:?}); use http://{ip_ap} na mao"),
    }
    log::warn!("[PROV] ===========================================");
    log::warn!("[PROV] MODO PROVISIONAMENTO ATIVO");
    log::warn!("[PROV]   1. Conecte um celular na rede:");
    log::warn!("[PROV]        SSID:  {ap_ssid_str}");
    log::warn!("[PROV]        Senha: {AP_PASSWORD}");
    log::warn!("[PROV]   2. Abra no browser: http://{ip_ap}");
    log::warn!("[PROV]   PIN de pareamento desta tomada: {}", ctx.pin_proprio);
    if let Some(m) = ctx.motivo {
        log::warn!("[PROV]   causa da reabertura: {}", m.banner());
    }
    log::warn!("[PROV] ===========================================");

    let (tx, rx) = mpsc::channel::<CredsRecebidas>();
    let html = montar_html_form(&redes, &ctx);
    let _server = subir_http_server(html, tx, ip_ap.clone(), &ctx)?;

    log::info!("[PROV] aguardando formulario (retry STA a cada 30 s se houver rede salva)...");

    // Laco unico: o formulario tem prioridade; no timeout, tenta a rede salva.
    // Tudo na MESMA thread — nao ha concorrencia sobre o driver Wi-Fi.
    loop {
        match rx.recv_timeout(INTERVALO_RETRY_STA) {
            Ok(creds) => {
                log::info!("[PROV] formulario recebido (modo={:?})", creds.form.modo);
                return Ok(SaidaPortal::Configurado(Box::new(creds)));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if ctx.ssid_salvo.is_none() {
                    continue; // nada salvo para tentar
                }
                if tentar_reconectar(wifi) {
                    log::warn!("[PROV] rede salva voltou -> encerrando portal e seguindo o boot");
                    return Ok(SaidaPortal::Reconectado);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(anyhow!("[PROV] canal do formulario fechou"));
            }
        }
    }
}

/// Uma tentativa de conexao STA com o AP no ar. `true` = conectou (FR-002).
fn tentar_reconectar(wifi: &mut BlockingWifi<EspWifi<'static>>) -> bool {
    log::info!("[PROV] tentando a rede salva em segundo plano...");
    if let Err(e) = wifi.connect() {
        log::debug!("[PROV] retry STA: connect falhou ({e:?})");
        return false;
    }
    match wifi.wait_netif_up() {
        Ok(()) => true,
        Err(e) => {
            log::debug!("[PROV] retry STA: sem IP ({e:?})");
            let _ = wifi.disconnect();
            false
        }
    }
}

// =============================================
// Configuracao de radio
// =============================================

/// AP sozinho (sem rede salva) ou AP+STA (com rede salva, para o retry do FR-002).
/// Netif do AP que faz o captive portal REALMENTE disparar.
///
/// O default do esp-idf-svc (`RouterConfiguration::default`) manda o servidor DHCP do AP
/// anunciar **8.8.8.8** como DNS. Com isso o celular manda toda consulta de nome para
/// 8.8.8.8 — que nao e nosso endereco, entao os pacotes NUNCA chegam ao listener de
/// `iniciar_dns_captive` em UDP:53, nenhum nome resolve para a tomada, e nenhum sistema
/// operacional detecta o portal. O DNS sequestrador existia e nunca recebia nada.
///
/// Anunciando o proprio IP do AP como DNS, as sondas de conectividade (Android
/// `connectivitycheck.gstatic.com`, iOS `captive.apple.com`, Windows `msftconnecttest.com`)
/// resolvem para a tomada, batem no handler HTTP, levam 302 e o aparelho abre a tela.
fn netif_ap_captive() -> Result<EspNetif> {
    let mut conf = NetifConfiguration::wifi_default_router();
    // Chave DIFERENTE da default: o netif original ainda existe quando criamos este, e
    // `esp_netif_new` recusa chave duplicada com ESP_ERR_INVALID_ARG.
    conf.key = "WIFI_AP_CAPT"
        .try_into()
        .map_err(|_| anyhow!("chave do netif AP invalida"))?;
    conf.ip_configuration = Some(IpConfiguration::Router(RouterConfiguration {
        subnet: Subnet {
            gateway: AP_IP,
            mask: Mask(24),
        },
        dhcp_enabled: true,
        dns: Some(AP_IP),
        // Sem secundario: um DNS alcancavel de verdade daria ao aparelho uma saida para
        // resolver os nomes das sondas e o portal deixaria de ser detectado.
        secondary_dns: None,
    }));
    EspNetif::new_with_conf(&conf).map_err(|e| anyhow!("netif AP do captive portal: {e:?}"))
}

fn configurar_ap_e_sta(
    wifi: &mut BlockingWifi<EspWifi<'static>>,
    ap_ssid: &str,
    ssid_salvo: Option<&str>,
) -> Result<()> {
    // Antes de qualquer configuracao: troca o netif do AP pelo que anuncia a propria
    // tomada como DNS (o driver ainda esta parado aqui).
    match netif_ap_captive().and_then(|n| {
        wifi.wifi_mut()
            .swap_netif_ap(n)
            .map_err(|e| anyhow!("swap_netif_ap falhou: {e:?}"))
    }) {
        Ok(_) => log::info!("[PROV] netif do AP: DNS anunciado por DHCP = {AP_IP}"),
        // Sem isso o portal ainda funciona digitando o IP na mao — nao vale abortar o boot.
        Err(e) => log::warn!("[PROV] captive portal degradado ({e:?}); use http://{AP_IP}"),
    }

    let ap = AccessPointConfiguration {
        ssid: ap_ssid
            .try_into()
            .map_err(|_| anyhow!("AP SSID muito longo"))?,
        ssid_hidden: false,
        channel: AP_CHANNEL,
        secondary_channel: None,
        auth_method: AuthMethod::WPA2Personal,
        password: AP_PASSWORD
            .try_into()
            .map_err(|_| anyhow!("AP password invalida"))?,
        max_connections: AP_MAX_CLIENTS,
        ..Default::default()
    };

    match ssid_salvo {
        Some(ssid) if !ssid.is_empty() => {
            // A senha vem da NVS via `EspWifi` (ja gravada); aqui basta o SSID e o
            // auth_method correto — senha vazia => rede aberta (FR-016).
            let cliente = super::wifi::client_config_salvo(ssid)?;
            wifi.set_configuration(&Configuration::Mixed(cliente, ap))?;
        }
        _ => {
            wifi.set_configuration(&Configuration::AccessPoint(ap))?;
        }
    }
    Ok(())
}

fn montar_ap_ssid(wifi: &BlockingWifi<EspWifi<'static>>) -> String {
    let mac = wifi.wifi().ap_netif().get_mac().unwrap_or([0; 6]);
    format!("Tomada-{:02X}{:02X}", mac[4], mac[5])
}

fn fazer_scan(wifi: &mut BlockingWifi<EspWifi<'static>>) -> Result<Vec<String>> {
    wifi.set_configuration(&Configuration::Client(ClientConfiguration::default()))?;
    wifi.start()?;
    let aps = wifi.scan()?;
    let _ = wifi.stop();

    let mut ssids: Vec<String> = aps
        .into_iter()
        .map(|ap| ap.ssid.as_str().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    ssids.sort();
    ssids.dedup();
    Ok(ssids)
}

// =============================================
// HTTP
// =============================================

fn subir_http_server(
    html: String,
    tx: mpsc::Sender<CredsRecebidas>,
    ip_ap: String,
    ctx: &ContextoPortal,
) -> Result<EspHttpServer<'static>> {
    // stack_size 10240: o default (6144) estoura ao formatar o HTML no handler.
    let mut server = EspHttpServer::new(&HttpServerConfig {
        uri_match_wildcard: true,
        stack_size: 10240,
        ..Default::default()
    })?;

    server.fn_handler::<anyhow::Error, _>("/*", Method::Get, move |req| {
        if req.uri() != "/" {
            let location = format!("http://{ip_ap}/");
            let headers = [("Location", location.as_str()), ("Content-Length", "0")];
            req.into_response(302, Some("Found"), &headers)?;
            return Ok(());
        }
        let mut resp = req.into_ok_response()?;
        resp.write_all(html.as_bytes())?;
        Ok(())
    })?;

    let tx_arc: Arc<Mutex<Option<mpsc::Sender<CredsRecebidas>>>> = Arc::new(Mutex::new(Some(tx)));
    let tx_clone = tx_arc.clone();
    let ssid_salvo = ctx.ssid_salvo.clone();
    let tem_nuvem = ctx.tem_identidade_nuvem;

    server.fn_handler::<anyhow::Error, _>("/save", Method::Post, move |mut req| {
        // --- corpo COMPLETO por Content-Length (FR-015) ---
        let content_length = req
            .header("Content-Length")
            .and_then(|v| v.trim().parse::<usize>().ok());

        let alvo = match portal::bytes_a_ler(content_length) {
            Ok(n) => n,
            Err(e) => return responder_erro(req, &e),
        };

        let mut corpo = Vec::with_capacity(alvo.min(1024));
        let mut buf = [0u8; 512];
        while corpo.len() < alvo {
            match req.read(&mut buf) {
                Ok(0) => break, // EOF
                Ok(n) => corpo.extend_from_slice(&buf[..n]),
                Err(e) => return Err(anyhow!("leitura do corpo falhou: {e:?}")),
            }
        }
        if let Err(e) = portal::checar_corpo(content_length, corpo.len()) {
            return responder_erro(req, &e);
        }

        let texto = String::from_utf8_lossy(&corpo).to_string();
        let form = match portal::parse_form(&texto, tem_nuvem) {
            Ok(f) => f,
            Err(e) => {
                log::warn!("[PROV] formulario rejeitado: {e}");
                return responder_erro(req, &e);
            }
        };
        let wifi = portal::resolver_wifi(&form, ssid_salvo.as_deref());
        log::info!(
            "[PROV] POST /save aceito (modo={:?}, wifi={:?})",
            form.modo,
            wifi
        );

        // Responde ANTES de sinalizar: o mini-browser do captive portal so
        // renderiza quando sabe que a pagina terminou (Content-Length + close).
        let corpo_resp = montar_html_resposta(&form);
        let len = corpo_resp.len().to_string();
        let headers = [
            ("Content-Type", "text/html; charset=utf-8"),
            ("Content-Length", len.as_str()),
            ("Connection", "close"),
        ];
        {
            let mut resp = req.into_response(200, Some("OK"), &headers)?;
            resp.write_all(corpo_resp.as_bytes())?;
            resp.flush()?;
        }

        // Uma submissao so (o sender e consumido).
        let mut guard = tx_clone.lock().unwrap();
        if let Some(s) = guard.take() {
            s.send(CredsRecebidas { form, wifi }).ok();
        }
        Ok(())
    })?;

    Ok(server)
}

/// 400/413 com a causa — nunca gravacao parcial (contracts/portal-ap.md).
fn responder_erro<C>(
    req: esp_idf_svc::http::server::Request<C>,
    e: &ErroForm,
) -> Result<(), anyhow::Error>
where
    C: embedded_svc::http::server::Connection,
{
    let status = portal::status_http(e);
    let corpo = montar_html_erro(&e.to_string());
    let len = corpo.len().to_string();
    let headers = [
        ("Content-Type", "text/html; charset=utf-8"),
        ("Content-Length", len.as_str()),
        ("Connection", "close"),
    ];
    let mut resp = req
        .into_response(status, None, &headers)
        .map_err(|e| anyhow!("resposta de erro falhou: {e:?}"))?;
    resp.write_all(corpo.as_bytes())
        .map_err(|e| anyhow!("escrita da resposta de erro falhou: {e:?}"))?;
    Ok(())
}

// =============================================
// DNS captive portal (inalterado do v1)
// =============================================

fn iniciar_dns_captive(ip: [u8; 4]) -> Result<()> {
    let sock = UdpSocket::bind("0.0.0.0:53").map_err(|e| anyhow!("bind UDP:53 falhou: {e:?}"))?;
    std::thread::Builder::new()
        .name("dns-captive".into())
        .stack_size(4096)
        .spawn(move || loop_dns(sock, ip))?;
    Ok(())
}

fn loop_dns(sock: UdpSocket, ip: [u8; 4]) {
    let mut buf = [0_u8; 512];
    loop {
        let (n, origem) = match sock.recv_from(&mut buf) {
            Ok(x) => x,
            Err(_) => continue,
        };
        if let Some(resp) = montar_resposta_dns(&buf[..n], ip) {
            let _ = sock.send_to(&resp, origem);
        }
    }
}

fn montar_resposta_dns(req: &[u8], ip: [u8; 4]) -> Option<Vec<u8>> {
    if req.len() < 12 {
        return None;
    }
    let q_start = 12;
    let mut i = q_start;
    while i < req.len() && req[i] != 0 {
        i += 1 + req[i] as usize;
    }
    let qname_end = i;
    let qtype_pos = qname_end + 1;
    if qtype_pos + 4 > req.len() {
        return None;
    }
    let qtype = u16::from_be_bytes([req[qtype_pos], req[qtype_pos + 1]]);
    let eh_a = qtype == 1;
    let q_end = qtype_pos + 4;

    let mut r = Vec::with_capacity(req.len() + 16);
    r.extend_from_slice(&req[0..2]);
    r.extend_from_slice(&[0x81, 0x80]);
    r.extend_from_slice(&[0x00, 0x01]);
    r.extend_from_slice(if eh_a { &[0x00, 0x01] } else { &[0x00, 0x00] });
    r.extend_from_slice(&[0x00, 0x00]);
    r.extend_from_slice(&[0x00, 0x00]);
    r.extend_from_slice(&req[q_start..q_end]);

    if eh_a {
        r.extend_from_slice(&[0xC0, 0x0C]);
        r.extend_from_slice(&[0x00, 0x01]);
        r.extend_from_slice(&[0x00, 0x01]);
        r.extend_from_slice(&[0x00, 0x00, 0x00, 0x3C]);
        r.extend_from_slice(&[0x00, 0x04]);
        r.extend_from_slice(&ip);
    }
    Some(r)
}

// =============================================
// HTML
// =============================================

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

const ESTILO: &str = "*{box-sizing:border-box}\
body{font-family:-apple-system,BlinkMacSystemFont,system-ui,sans-serif;background:#f5f5f7;color:#1d1d1f;margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;padding:16px}\
.card{background:#fff;padding:28px;border-radius:16px;box-shadow:0 4px 24px rgba(0,0,0,.08);max-width:440px;width:100%}\
h1{margin:0 0 4px;font-size:22px}.sub{color:#6e6e73;margin:0 0 18px;font-size:14px}\
label{display:block;margin:12px 0 6px;font-size:13px;font-weight:500}\
select,input[type=text],input[type=password]{width:100%;padding:11px;border:1px solid #d2d2d7;border-radius:10px;font-size:15px;background:#fff;font-family:inherit}\
input:focus,select:focus{outline:none;border-color:#5856d6;box-shadow:0 0 0 3px rgba(88,86,214,.15)}\
button{width:100%;padding:14px;margin-top:20px;border:0;border-radius:12px;background:#5856d6;color:#fff;font-size:15px;font-weight:600;cursor:pointer}\
button:hover{background:#4a48c2}\
.aviso{background:#fff4e5;border:1px solid #ffd8a8;color:#8a5300;padding:12px;border-radius:10px;font-size:13px;margin-bottom:16px}\
.erro{background:#ffe9e9;border:1px solid #ffc9c9;color:#a12626;padding:12px;border-radius:10px;font-size:14px}\
.help{color:#86868b;font-size:12px;margin:6px 0 0}\
.modos{display:flex;gap:8px;margin:8px 0 4px;flex-wrap:wrap}\
.modos label{flex:1;min-width:120px;margin:0;border:1px solid #d2d2d7;border-radius:10px;padding:10px;font-size:13px;cursor:pointer;text-align:center}\
.modos input{margin-right:6px}\
fieldset{border:1px solid #e5e5ea;border-radius:10px;padding:12px;margin:14px 0 0}\
legend{font-size:12px;color:#6e6e73;padding:0 6px}\
.pin{font-family:ui-monospace,monospace;font-size:20px;letter-spacing:.15em;background:#f5f5f7;padding:8px 12px;border-radius:8px;display:inline-block}";

fn montar_html_form(redes: &[String], ctx: &ContextoPortal) -> String {
    let banner = match ctx.motivo {
        Some(m) => format!("<div class=\"aviso\">{}</div>", html_escape(m.banner())),
        None => String::new(),
    };

    let opcoes: String = redes
        .iter()
        .take(20)
        .map(|s| format!("<option value=\"{0}\">{0}</option>", html_escape(s)))
        .collect();
    let secao_scan = if redes.is_empty() {
        String::new()
    } else {
        format!(
            "<label for=\"lista\">Redes encontradas</label>\
<select id=\"lista\" onchange=\"document.getElementById('ssid').value=this.value\">\
<option value=\"\">-- escolha --</option>{opcoes}</select>"
        )
    };

    // Preserva o Wi-Fi salvo: SSID pre-preenchido + checkbox "manter" (FR-017).
    let (ssid_val, bloco_manter) = match &ctx.ssid_salvo {
        Some(s) => (
            html_escape(s),
            format!(
                "<label><input type=\"checkbox\" name=\"manter\" value=\"1\" checked> \
Manter a rede salva (<b>{}</b>)</label>\
<p class=\"help\">Deixe a senha em branco para manter a atual; preencha para trocar a senha.</p>",
                html_escape(s)
            ),
        ),
        None => (String::new(), String::new()),
    };

    format!(
        r#"<!DOCTYPE html><html lang="pt-BR"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Configurar Tomada Inteligente</title><style>{ESTILO}</style></head><body>
<div class="card">
<h1>Tomada Inteligente</h1><p class="sub">Configure esta tomada</p>
{banner}
<form action="/save" method="POST" onsubmit="return validar()">

<fieldset><legend>Rede Wi-Fi</legend>
{secao_scan}
<label for="ssid">Nome da rede (SSID)</label>
<input id="ssid" name="ssid" type="text" value="{ssid_val}" placeholder="Nome da rede" maxlength="32" autocomplete="off">
<label for="pwd">Senha</label>
<input id="pwd" name="pwd" type="password" placeholder="Deixe vazio para rede aberta" maxlength="63" autocomplete="off">
{bloco_manter}
</fieldset>

<fieldset><legend>Ativacao</legend>
<label for="claim">Codigo de ativacao</label>
<input id="claim" name="claim" type="text" placeholder="Ex.: K7PQ2XMR" maxlength="64" autocomplete="off" autocapitalize="characters">
<p class="help">Gere no dashboard (Residencia &rarr; Parear dispositivo). Vale ~10 min, uso unico.</p>

<details>
<summary>Avancado</summary>
<label for="servidor">Endereco do servidor</label>
<input id="servidor" name="servidor" type="text" value="{servidor}" placeholder="http://192.168.0.10:8080" maxlength="128" autocomplete="off">
<p class="help">Ja preenchido com o servidor deste firmware. So mude se o endereco do servidor for outro.</p>
</details>
</fieldset>

<button type="submit">Salvar</button>
</form></div>
<script>
function bytes(s){{return new TextEncoder().encode(s).length;}}
function validar(){{
  var s=document.getElementById('ssid').value;
  if(bytes(s)>{MAX_SSID_BYTES}){{
    alert('O nome da rede tem '+bytes(s)+' bytes (maximo {MAX_SSID_BYTES}). Acentos contam 2.');
    return false;
  }}
  return true;
}}
</script>
</body></html>"#,
        servidor = html_escape(&ctx.servidor_default),
    )
}

fn montar_html_resposta(form: &FormPortal) -> String {
    let msg = match form.modo {
        Modo::Nuvem => format!(
            "A tomada vai conectar em <b>{}</b> e se registrar no servidor apos reiniciar.",
            html_escape(&form.ssid)
        ),
        Modo::Standalone => {
            "A tomada vai operar sem nuvem. Ela ja comeca a medir e controlar apos reiniciar."
                .to_string()
        }
        Modo::Parear => {
            "Procurando a outra tomada... o resultado aparece no console e a tomada reinicia."
                .to_string()
        }
    };
    format!(
        r#"<!DOCTYPE html><html lang="pt-BR"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1"><title>Salvo</title>
<style>{ESTILO}</style></head><body><div class="card" style="text-align:center">
<div style="font-size:44px;color:#22c55e">&#10003;</div>
<h1 style="color:#22c55e">Configuracao salva</h1><p class="sub">{msg}<br>Voce pode fechar esta pagina.</p>
</div></body></html>"#
    )
}

fn montar_html_erro(motivo: &str) -> String {
    format!(
        r#"<!DOCTYPE html><html lang="pt-BR"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1"><title>Erro</title>
<style>{ESTILO}</style></head><body><div class="card">
<h1>Nao deu para salvar</h1>
<div class="erro">{}</div>
<p class="help">Nada foi alterado na tomada. Volte e corrija o campo indicado.</p>
<button onclick="history.back()">Voltar</button>
</div></body></html>"#,
        html_escape(motivo)
    )
}
