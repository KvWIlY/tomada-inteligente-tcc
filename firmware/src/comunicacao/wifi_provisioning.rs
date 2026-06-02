//! Modo provisioning -- ESP32 vira AP + HTTP server pra receber credenciais.
//!
//! Fluxo:
//! 1. Scan de redes proximas (em modo STA temporario)
//! 2. Reconfigura como AP "Tomada-XXXX" senha "tomadatcc"
//! 3. Sobe HTTP server na porta 80
//! 4. GET / -> serve HTML form com lista de redes
//! 5. POST /save -> recebe SSID+senha, sinaliza thread principal via channel
//! 6. Bloqueia ate receber, retorna as credenciais

use anyhow::{anyhow, Result};
use embedded_svc::io::Write;
use embedded_svc::io::Read as _;
use esp_idf_svc::{
    http::{
        server::{Configuration as HttpServerConfig, EspHttpServer},
        Method,
    },
    wifi::{
        AccessPointConfiguration, AuthMethod, BlockingWifi, ClientConfiguration,
        Configuration, EspWifi,
    },
};
use std::net::UdpSocket;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

const AP_PASSWORD: &str = "tomadatcc";
const AP_CHANNEL: u8 = 1;
const AP_MAX_CLIENTS: u16 = 4;

pub struct CredsRecebidas {
    pub ssid: String,
    pub pwd: String,
    /// Codigo de pareamento (claim-code) digitado no portal. Vazio se nao informado.
    /// O firmware o usa UMA vez em `provisionar`; o device nunca cunha o proprio claim
    /// (Principio VIII — cunhar exige service_role/dono, ver scripts/criar-claim.ps1).
    pub claim_code: String,
}

pub fn iniciar(wifi: &mut BlockingWifi<EspWifi<'static>>) -> Result<CredsRecebidas> {
    // 1. Scan de redes (STA temporario, sem conectar)
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

    // 2. Configura como AP
    let ap_ssid_str = montar_ap_ssid(wifi);
    let ap_ssid_cfg = ap_ssid_str
        .as_str()
        .try_into()
        .map_err(|_| anyhow!("AP SSID muito longo"))?;
    let ap_pwd_cfg = AP_PASSWORD
        .try_into()
        .map_err(|_| anyhow!("AP password invalida"))?;

    wifi.set_configuration(&Configuration::AccessPoint(AccessPointConfiguration {
        ssid: ap_ssid_cfg,
        ssid_hidden: false,
        channel: AP_CHANNEL,
        secondary_channel: None,
        auth_method: AuthMethod::WPA2Personal,
        password: ap_pwd_cfg,
        max_connections: AP_MAX_CLIENTS,
        ..Default::default()
    }))?;
    wifi.start()?;

    // IP real do AP (varia conforme o sdkconfig; nao assumir 192.168.4.1).
    let ip_octets = wifi
        .wifi()
        .ap_netif()
        .get_ip_info()
        .map(|i| i.ip.octets())
        .unwrap_or([192, 168, 4, 1]);
    let ip_ap = format!(
        "{}.{}.{}.{}",
        ip_octets[0], ip_octets[1], ip_octets[2], ip_octets[3]
    );

    // Captive portal: DNS hijack -> todo dominio resolve no IP do AP. Assim a checagem
    // de internet do celular (ex.: connectivitycheck.gstatic.com) cai no nosso servidor,
    // que serve o form -> Android/iOS mostram "entrar na rede" e NAO fogem pro 4G/rede
    // de casa (era o que impedia o POST /save de chegar).
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
    log::warn!("[PROV]   3. Digite SSID e senha da rede de casa");
    log::warn!("[PROV] ===========================================");

    // 3. Sobe HTTP server e bloqueia ate receber form
    let (tx, rx) = mpsc::channel::<CredsRecebidas>();
    let _server = subir_http_server(redes, tx)?; // mantem vivo enquanto _server existir

    log::info!("[PROV] aguardando credenciais via formulario HTTP...");

    let creds = rx
        .recv()
        .map_err(|e| anyhow!("[PROV] canal fechado sem receber creds: {e:?}"))?;

    log::info!("[PROV] credenciais recebidas (SSID={})", creds.ssid);
    Ok(creds)
}

// =============================================
// Helpers privados
// =============================================

fn montar_ap_ssid(wifi: &BlockingWifi<EspWifi<'static>>) -> String {
    let mac = wifi.wifi().ap_netif().get_mac().unwrap_or([0; 6]);
    format!("Tomada-{:02X}{:02X}", mac[4], mac[5])
}

fn fazer_scan(wifi: &mut BlockingWifi<EspWifi<'static>>) -> Result<Vec<String>> {
    // Configura como STA pra fazer scan (precisa estar startado em alguma config)
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

fn subir_http_server(
    redes: Vec<String>,
    tx: mpsc::Sender<CredsRecebidas>,
) -> Result<EspHttpServer<'static>> {
    // uri_match_wildcard: deixa um unico handler GET "/*" responder QUALQUER caminho.
    let mut server = EspHttpServer::new(&HttpServerConfig {
        uri_match_wildcard: true,
        ..Default::default()
    })?;
    let html = montar_html_form(&redes);

    // GET /* -> serve o formulario para qualquer caminho (captive portal). As URLs de
    // checagem de internet do SO (/generate_204, /hotspot-detect.html, /ncsi.txt...)
    // caem aqui em vez do 204/sucesso esperado -> o celular abre o portal sozinho.
    server.fn_handler::<anyhow::Error, _>("/*", Method::Get, move |req| {
        let mut resp = req.into_ok_response()?;
        resp.write_all(html.as_bytes())?;
        Ok(())
    })?;

    // POST /save -> recebe form, sinaliza thread principal
    let tx_arc: Arc<Mutex<Option<mpsc::Sender<CredsRecebidas>>>> = Arc::new(Mutex::new(Some(tx)));
    let tx_clone = tx_arc.clone();
    server.fn_handler::<anyhow::Error, _>("/save", Method::Post, move |mut req| {
        let mut buf = vec![0_u8; 512];
        let len = req.read(&mut buf)?;
        let body = std::str::from_utf8(&buf[..len])
            .map_err(|e| anyhow!("body nao e UTF-8: {e:?}"))?;

        let (ssid, pwd, claim_code) = parse_form_body(body)?;
        log::info!("[PROV] POST /save recebido (SSID={ssid}, claim={}chars)", claim_code.len());

        // Responde ANTES de sinalizar o reboot. Content-Length explicito +
        // flush + drop garantem que o celular receba a resposta inteira: o
        // mini-browser de captive portal so renderiza a pagina quando sabe que
        // ela terminou (sem isso, o AP cai no restart antes de renderizar).
        let resp_html = montar_html_resposta(&ssid);
        let content_len = resp_html.len().to_string();
        let headers = [
            ("Content-Type", "text/html; charset=utf-8"),
            ("Content-Length", content_len.as_str()),
            ("Connection", "close"),
        ];
        {
            let mut resp = req.into_response(200, Some("OK"), &headers)?;
            resp.write_all(resp_html.as_bytes())?;
            resp.flush()?;
        } // resp dropado aqui -> resposta finalizada e empurrada pro celular

        // Sinaliza thread principal (consume o sender pra so disparar 1 vez)
        let mut guard = tx_clone.lock().unwrap();
        if let Some(s) = guard.take() {
            s.send(CredsRecebidas { ssid, pwd, claim_code }).ok();
        }

        Ok(())
    })?;

    Ok(server)
}

// =============================================
// DNS captive portal (UDP:53 -> responde tudo com o IP do AP)
// =============================================

/// Sobe um mini-servidor DNS na porta 53 que responde QUALQUER consulta A com o IP do AP.
/// E o que faz o celular tratar a rede como portal e parar de fugir pro 4G/rede de casa.
fn iniciar_dns_captive(ip: [u8; 4]) -> Result<()> {
    let sock = UdpSocket::bind("0.0.0.0:53")
        .map_err(|e| anyhow!("bind UDP:53 falhou: {e:?}"))?;
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

/// Monta uma resposta DNS apontando o nome consultado para `ip` (registro A, TTL 60s).
/// Responde so a consultas A (qtype=1); outras voltam sem resposta (ANCOUNT=0).
fn montar_resposta_dns(req: &[u8], ip: [u8; 4]) -> Option<Vec<u8>> {
    if req.len() < 12 {
        return None; // header DNS tem 12 bytes
    }
    // Fim do QNAME (sequencia de labels terminada em 0x00).
    let q_start = 12;
    let mut i = q_start;
    while i < req.len() && req[i] != 0 {
        i += 1 + req[i] as usize;
    }
    let qname_end = i; // posicao do byte 0x00 terminador
    let qtype_pos = qname_end + 1;
    if qtype_pos + 4 > req.len() {
        return None; // sem QTYPE/QCLASS completos
    }
    let qtype = u16::from_be_bytes([req[qtype_pos], req[qtype_pos + 1]]);
    let eh_a = qtype == 1; // 1 = registro A (IPv4)
    let q_end = qtype_pos + 4; // inclui QTYPE(2) + QCLASS(2)

    let mut r = Vec::with_capacity(req.len() + 16);
    r.extend_from_slice(&req[0..2]); // ID (ecoa)
    r.extend_from_slice(&[0x81, 0x80]); // flags: resposta, RD+RA, sem erro
    r.extend_from_slice(&[0x00, 0x01]); // QDCOUNT = 1
    r.extend_from_slice(if eh_a { &[0x00, 0x01] } else { &[0x00, 0x00] }); // ANCOUNT
    r.extend_from_slice(&[0x00, 0x00]); // NSCOUNT
    r.extend_from_slice(&[0x00, 0x00]); // ARCOUNT
    r.extend_from_slice(&req[q_start..q_end]); // ecoa a pergunta

    if eh_a {
        r.extend_from_slice(&[0xC0, 0x0C]); // ponteiro p/ o QNAME (offset 12)
        r.extend_from_slice(&[0x00, 0x01]); // TYPE A
        r.extend_from_slice(&[0x00, 0x01]); // CLASS IN
        r.extend_from_slice(&[0x00, 0x00, 0x00, 0x3C]); // TTL = 60s
        r.extend_from_slice(&[0x00, 0x04]); // RDLENGTH = 4
        r.extend_from_slice(&ip); // RDATA = IP do AP
    }
    Some(r)
}

fn parse_form_body(body: &str) -> Result<(String, String, String)> {
    let mut ssid: Option<String> = None;
    let mut pwd: Option<String> = None;
    let mut claim: Option<String> = None;
    for pair in body.split('&') {
        let mut it = pair.splitn(2, '=');
        let key = it.next().unwrap_or("");
        let val = it.next().unwrap_or("");
        let val_decoded = url_decode(val);
        match key {
            "ssid" => ssid = Some(val_decoded),
            "pwd" => pwd = Some(val_decoded),
            "claim" => claim = Some(val_decoded),
            _ => {}
        }
    }
    let ssid = ssid.ok_or_else(|| anyhow!("campo 'ssid' faltando no form"))?;
    if ssid.is_empty() {
        return Err(anyhow!("ssid vazio"));
    }
    let pwd = pwd.unwrap_or_default();
    let claim = claim.unwrap_or_default().trim().to_string();
    Ok((ssid, pwd, claim))
}

/// URL decoder simples: %XX e + -> espaco.
fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let h = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("00");
                let c = u8::from_str_radix(h, 16).unwrap_or(b'?');
                out.push(c);
                i += 3;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).unwrap_or_default()
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

fn montar_html_form(redes: &[String]) -> String {
    let opcoes_redes: String = redes
        .iter()
        .take(20)
        .map(|s| {
            format!(
                "<option value=\"{0}\">{0}</option>",
                html_escape(s)
            )
        })
        .collect();

    let secao_scan = if redes.is_empty() {
        String::new()
    } else {
        format!(
            r#"<label for="ssid-list">Redes encontradas</label>
<select id="ssid-list" onchange="document.getElementById('ssid').value=this.value">
<option value="">-- escolha uma rede --</option>
{opcoes_redes}
</select>
<div class="divider">ou digite manualmente</div>"#
        )
    };

    format!(
        r#"<!DOCTYPE html>
<html lang="pt-BR">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Configurar Wi-Fi -- Tomada Inteligente</title>
<style>
*{{box-sizing:border-box}}
body{{font-family:-apple-system,BlinkMacSystemFont,system-ui,sans-serif;background:#f5f5f7;color:#1d1d1f;margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;padding:16px}}
.card{{background:white;padding:32px;border-radius:16px;box-shadow:0 4px 24px rgba(0,0,0,0.08);max-width:420px;width:100%}}
h1{{margin:0 0 4px;font-size:22px}}
.sub{{color:#6e6e73;margin:0 0 24px;font-size:14px}}
label{{display:block;margin:14px 0 6px;font-size:13px;font-weight:500;color:#1d1d1f}}
select,input{{width:100%;padding:12px;border:1px solid #d2d2d7;border-radius:10px;font-size:15px;background:white;color:#1d1d1f;font-family:inherit}}
input:focus,select:focus{{outline:none;border-color:#5856d6;box-shadow:0 0 0 3px rgba(88,86,214,0.15)}}
button{{width:100%;padding:14px;margin-top:20px;border:0;border-radius:12px;background:#5856d6;color:white;font-size:15px;font-weight:600;cursor:pointer;transition:background .15s}}
button:hover{{background:#4a48c2}}
button:active{{transform:scale(0.98)}}
.divider{{text-align:center;color:#6e6e73;font-size:12px;margin:14px 0 4px;text-transform:uppercase;letter-spacing:0.05em}}
.help{{color:#86868b;font-size:12px;margin-top:6px}}
</style>
</head>
<body>
<div class="card">
<h1>Tomada Inteligente</h1>
<p class="sub">Configure a conexao Wi-Fi da sua tomada</p>
<form action="/save" method="POST">
{secao_scan}
<label for="ssid">SSID</label>
<input id="ssid" name="ssid" type="text" placeholder="Nome da rede" required maxlength="32" autocomplete="off">
<label for="pwd">Senha</label>
<input id="pwd" name="pwd" type="password" placeholder="Senha do Wi-Fi" maxlength="63" autocomplete="off">
<div class="divider">pareamento</div>
<label for="claim">Codigo de pareamento</label>
<input id="claim" name="claim" type="text" placeholder="Ex.: TOMADA-TESTE-001" maxlength="64" autocomplete="off" autocapitalize="characters">
<p class="help">Pegue o codigo com o dono da residencia (gerado via scripts/criar-claim). Necessario so no 1o uso.</p>
<p class="help">A tomada vai reiniciar automaticamente apos salvar.</p>
<button type="submit">Conectar e salvar</button>
</form>
</div>
</body>
</html>"#
    )
}

fn montar_html_resposta(ssid: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="pt-BR">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Salvo</title>
<style>
body{{font-family:-apple-system,system-ui,sans-serif;background:#f5f5f7;color:#1d1d1f;display:flex;align-items:center;justify-content:center;min-height:100vh;margin:0;padding:16px;text-align:center}}
.card{{background:white;padding:40px 32px;border-radius:16px;max-width:380px;box-shadow:0 4px 24px rgba(0,0,0,0.08)}}
.check{{font-size:48px;color:#22c55e;margin-bottom:8px}}
h1{{margin:0 0 12px;font-size:22px;color:#22c55e}}
p{{color:#6e6e73;font-size:14px;line-height:1.5}}
b{{color:#1d1d1f}}
</style>
</head>
<body>
<div class="card">
<div class="check">&#10003;</div>
<h1>Configuracao salva</h1>
<p>A tomada vai conectar em <b>{}</b> apos reiniciar.<br>
Voce pode fechar esta pagina.</p>
</div>
</body>
</html>"#,
        html_escape(ssid)
    )
}
