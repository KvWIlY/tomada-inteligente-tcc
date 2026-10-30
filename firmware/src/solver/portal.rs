//! Logica PURA do Portal AP v2 (feature 005, `contracts/portal-ap.md`).
//!
//! Fica aqui - e nao no firmware - porque e a parte do portal que tem regra de negocio
//! e casos de borda de verdade (framing por `Content-Length`, SSID em BYTES, matriz
//! `manter` x senha) e que precisa rodar no host (Principio IX). O firmware so faz o
//! I/O: le do socket, chama `parse_form`, aplica `ResolucaoWifi`.
//!
//! Nada aqui aloca conexao, toca NVS ou depende de `esp-idf`.

use std::fmt;

/// Teto do corpo do POST /save (FR-015). Acima disso o portal responde 413 em vez de
/// truncar em silencio (o bug do v1: um unico `read` de 512 B).
pub const CAP_CORPO: usize = 2048;

/// Limite de SSID em BYTES (nao chars) - e o limite do radio (FR-018).
pub const MAX_SSID_BYTES: usize = 32;
/// Limite de senha WPA2 em bytes.
pub const MAX_SENHA_BYTES: usize = 63;
/// Tamanho do PIN de pareamento (contracts/pareamento-espnow.md).
pub const TAM_PIN: usize = 8;
/// Limite do endereco do servidor.
pub const MAX_SERVIDOR_BYTES: usize = 128;

/// Modo escolhido no formulario (radio `modo`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modo {
    /// Provisionar contra o backend (servidor + claim-code).
    Nuvem,
    /// Operar sem nuvem: identidade local (FR-004).
    Standalone,
    /// Receber a chave da casa de uma tomada existente (FR-008).
    Parear,
}

impl Modo {
    fn parse(s: &str) -> Option<Modo> {
        match s {
            "nuvem" => Some(Modo::Nuvem),
            "standalone" => Some(Modo::Standalone),
            "parear" => Some(Modo::Parear),
            _ => None,
        }
    }
}

/// Causa da reabertura do portal (NVS `motivo_portal`, uso unico - FR-019).
/// Taxonomia ESTAVEL: o firmware grava a variante, o form renderiza o banner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotivoPortal {
    /// Falha de autenticacao no STA. Quando o radio nao distingue de "rede fora de
    /// alcance", o banner e a mensagem COMBINADA (FR-019 pos-checklist).
    SenhaWifi,
    /// Claim-code invalido/expirado/usado (401/403 do backend).
    ClaimInvalido,
    /// N falhas transitorias consecutivas com provisionamento pendente (FR-019a).
    ServidorInacessivel,
    /// SSID salvo nao cabe no limite do radio.
    SsidInvalido,
}

impl MotivoPortal {
    /// Chave persistida na NVS (estavel - nao renomear sem migrar).
    pub fn as_str(self) -> &'static str {
        match self {
            MotivoPortal::SenhaWifi => "senha_wifi",
            MotivoPortal::ClaimInvalido => "claim_invalido",
            MotivoPortal::ServidorInacessivel => "servidor_inacessivel",
            MotivoPortal::SsidInvalido => "ssid_invalido",
        }
    }

    pub fn parse(s: &str) -> Option<MotivoPortal> {
        match s {
            "senha_wifi" => Some(MotivoPortal::SenhaWifi),
            "claim_invalido" => Some(MotivoPortal::ClaimInvalido),
            "servidor_inacessivel" => Some(MotivoPortal::ServidorInacessivel),
            "ssid_invalido" => Some(MotivoPortal::SsidInvalido),
            _ => None,
        }
    }

    /// Texto exibido no topo do formulario (pt-BR).
    pub fn banner(self) -> &'static str {
        match self {
            // Honestidade sobre o que o radio entrega: reason codes de desassociacao
            // nao separam com seguranca senha errada de rede fora de alcance.
            MotivoPortal::SenhaWifi => "Nao foi possivel entrar na rede: senha incorreta ou rede indisponivel.",
            MotivoPortal::ClaimInvalido => "O codigo de pareamento e invalido, expirado ou ja foi usado. Gere um novo no dashboard.",
            MotivoPortal::ServidorInacessivel => "O servidor nao respondeu. Confira o endereco e se ele esta ligado na mesma rede.",
            MotivoPortal::SsidInvalido => "O nome da rede salva e invalido (excede 32 bytes). Escolha a rede novamente.",
        }
    }
}

/// Erro de validacao do formulario. O portal responde 400 (ou 413) SEM gravar nada -
/// nunca ha gravacao parcial (contracts/portal-ap.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErroForm {
    /// Corpo maior que `CAP_CORPO` -> HTTP 413.
    CorpoGrande { tamanho: usize },
    /// `Content-Length` prometeu mais bytes do que chegaram (conexao cortada).
    CorpoIncompleto { esperado: usize, recebido: usize },
    SsidVazio,
    /// Contagem em BYTES (FR-018) - SSID com acento estoura antes de 32 chars.
    SsidLongo { bytes: usize },
    SenhaLonga { bytes: usize },
    ModoInvalido,
    ClaimInvalido,
    PinInvalido,
    ServidorInvalido,
    /// `modo=standalone` sobre identidade de nuvem sem a caixa de confirmacao.
    ConfirmacaoFaltando,
}

impl fmt::Display for ErroForm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErroForm::CorpoGrande { tamanho } => {
                write!(f, "formulario grande demais ({tamanho} bytes; maximo {CAP_CORPO})")
            }
            ErroForm::CorpoIncompleto { esperado, recebido } => {
                write!(f, "envio incompleto ({recebido} de {esperado} bytes)")
            }
            ErroForm::SsidVazio => write!(f, "informe o nome da rede (SSID)"),
            ErroForm::SsidLongo { bytes } => write!(
                f,
                "nome da rede tem {bytes} bytes (maximo {MAX_SSID_BYTES}); acentos contam 2"
            ),
            ErroForm::SenhaLonga { bytes } => {
                write!(f, "senha tem {bytes} bytes (maximo {MAX_SENHA_BYTES})")
            }
            ErroForm::ModoInvalido => write!(f, "modo invalido"),
            ErroForm::ClaimInvalido => write!(f, "codigo de pareamento deve ter 6 a 64 caracteres"),
            ErroForm::PinInvalido => {
                write!(f, "PIN deve ter {TAM_PIN} caracteres alfanumericos")
            }
            ErroForm::ServidorInvalido => {
                write!(f, "endereco do servidor invalido (use http://IP:PORTA)")
            }
            ErroForm::ConfirmacaoFaltando => write!(
                f,
                "confirme o descarte da identidade de nuvem para usar o modo sem nuvem"
            ),
        }
    }
}

/// Status HTTP que o firmware deve responder para este erro.
pub fn status_http(e: &ErroForm) -> u16 {
    match e {
        ErroForm::CorpoGrande { .. } => 413,
        _ => 400,
    }
}

/// Formulario validado. `ssid`/`pwd` ainda passam por `resolver_wifi`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormPortal {
    pub ssid: String,
    pub pwd: String,
    /// Checkbox "manter rede salva".
    pub manter: bool,
    pub modo: Modo,
    /// So em `Modo::Nuvem`.
    pub servidor: String,
    /// So em `Modo::Nuvem`.
    pub claim: String,
    /// So em `Modo::Parear` - PIN da tomada EXISTENTE.
    pub pin: String,
}

/// Quantos bytes ler do socket, dado o `Content-Length` anunciado (FR-015).
/// `None` (sem header) => ler ate EOF, limitado pelo cap.
pub fn bytes_a_ler(content_length: Option<usize>) -> Result<usize, ErroForm> {
    match content_length {
        Some(n) if n > CAP_CORPO => Err(ErroForm::CorpoGrande { tamanho: n }),
        Some(n) => Ok(n),
        None => Ok(CAP_CORPO),
    }
}

/// Confere o que foi efetivamente lido contra o prometido pelo `Content-Length`.
/// Existe porque TCP entrega em pedacos: o firmware precisa de um laco de `read` e
/// de um criterio de parada - o bug do v1 era um unico `read` de 512 B.
pub fn checar_corpo(content_length: Option<usize>, recebido: usize) -> Result<(), ErroForm> {
    if recebido > CAP_CORPO {
        return Err(ErroForm::CorpoGrande { tamanho: recebido });
    }
    match content_length {
        Some(n) if recebido < n => Err(ErroForm::CorpoIncompleto {
            esperado: n,
            recebido,
        }),
        _ => Ok(()),
    }
}

/// Decodifica `application/x-www-form-urlencoded` (`%XX` e `+`).
pub fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            // `i + 2 < len` do v1 descartava um %XX no ULTIMO byte do corpo.
            b'%' if i + 2 < bytes.len() + 1 && i + 3 <= bytes.len() => {
                let h = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(h, 16) {
                    Ok(c) => {
                        out.push(c);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn campo<'a>(pares: &'a [(String, String)], chave: &str) -> Option<&'a str> {
    pares
        .iter()
        .find(|(k, _)| k == chave)
        .map(|(_, v)| v.as_str())
}

/// Valida o endereco do servidor: `http(s)://host[:porta][/...]`, sem espacos.
pub fn servidor_valido(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() || s.len() > MAX_SERVIDOR_BYTES || s.contains(char::is_whitespace) {
        return false;
    }
    let resto = match s.strip_prefix("http://").or_else(|| s.strip_prefix("https://")) {
        Some(r) => r,
        None => return false,
    };
    // Precisa de host antes de `/` ou `:`.
    !resto.is_empty() && !resto.starts_with('/') && !resto.starts_with(':')
}

/// Parseia e valida o corpo do `POST /save` (form v2).
///
/// `tem_identidade_nuvem`: se a tomada ja foi provisionada - `modo=standalone` entao
/// exige a caixa de confirmacao (Edge Case da spec).
pub fn parse_form(body: &str, tem_identidade_nuvem: bool) -> Result<FormPortal, ErroForm> {
    let pares: Vec<(String, String)> = body
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut it = p.splitn(2, '=');
            let k = it.next().unwrap_or("");
            let v = it.next().unwrap_or("");
            (url_decode(k), url_decode(v))
        })
        .collect();

    let modo = Modo::parse(campo(&pares, "modo").unwrap_or("nuvem").trim())
        .ok_or(ErroForm::ModoInvalido)?;
    let manter = matches!(campo(&pares, "manter"), Some("1") | Some("on"));

    let ssid = campo(&pares, "ssid").unwrap_or("").trim().to_string();
    // SSID so e obrigatorio no modo NUVEM (o unico que precisa alcancar o servidor) e
    // apenas sem "manter" - com "manter" o firmware usa o salvo (`resolver_wifi`).
    // Standalone e Parear funcionam sem Wi-Fi nenhum: e exatamente o caso da casa sem
    // internet, o coracao desta feature.
    if !manter && ssid.is_empty() && modo == Modo::Nuvem {
        return Err(ErroForm::SsidVazio);
    }
    if ssid.len() > MAX_SSID_BYTES {
        return Err(ErroForm::SsidLongo { bytes: ssid.len() });
    }

    let pwd = campo(&pares, "pwd").unwrap_or("").to_string();
    if pwd.len() > MAX_SENHA_BYTES {
        return Err(ErroForm::SenhaLonga { bytes: pwd.len() });
    }

    let mut servidor = String::new();
    let mut claim = String::new();
    let mut pin = String::new();

    match modo {
        Modo::Nuvem => {
            servidor = campo(&pares, "servidor").unwrap_or("").trim().to_string();
            if !servidor_valido(&servidor) {
                return Err(ErroForm::ServidorInvalido);
            }
            claim = campo(&pares, "claim").unwrap_or("").trim().to_string();
            if claim.len() < 6 || claim.len() > 64 {
                return Err(ErroForm::ClaimInvalido);
            }
        }
        Modo::Standalone => {
            if tem_identidade_nuvem
                && !matches!(
                    campo(&pares, "confirmar_descartar_identidade"),
                    Some("1") | Some("on")
                )
            {
                return Err(ErroForm::ConfirmacaoFaltando);
            }
        }
        Modo::Parear => {
            pin = campo(&pares, "pin").unwrap_or("").trim().to_uppercase();
            if pin.len() != TAM_PIN || !pin.chars().all(|c| c.is_ascii_alphanumeric()) {
                return Err(ErroForm::PinInvalido);
            }
        }
    }

    Ok(FormPortal {
        ssid,
        pwd,
        manter,
        modo,
        servidor,
        claim,
        pin,
    })
}

/// O que gravar na NVS de Wi-Fi depois de um formulario valido (FR-016/017).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolucaoWifi {
    /// Nao mexer na NVS (mantem SSID e senha salvos).
    Preservar,
    /// Gravar SSID e senha. `senha` vazia => rede aberta (`AuthMethod::None`).
    Gravar { ssid: String, senha: String },
    /// Manter o SSID salvo e trocar SO a senha (mesmo roteador, senha nova).
    TrocarSenha { senha: String },
    /// Nenhuma rede informada e nenhuma salva (standalone sem Wi-Fi).
    SemRede,
}

/// Matriz `manter` x senha x SSID (CHK018):
///   manter + senha vazia            -> Preservar
///   manter + senha preenchida       -> TrocarSenha (SSID salvo continua)
///   manter + SSID novo digitado     -> Gravar (o usuario sobrepos a escolha)
///   sem manter + SSID               -> Gravar (senha vazia = rede aberta)
///   sem manter, sem SSID, sem salvo -> SemRede
pub fn resolver_wifi(form: &FormPortal, ssid_salvo: Option<&str>) -> ResolucaoWifi {
    let ssid_digitado = !form.ssid.is_empty();
    let senha_digitada = !form.pwd.is_empty();

    if form.manter && ssid_salvo.is_some() {
        // SSID digitado diferente do salvo => o usuario quis trocar de rede mesmo.
        if ssid_digitado && Some(form.ssid.as_str()) != ssid_salvo {
            return ResolucaoWifi::Gravar {
                ssid: form.ssid.clone(),
                senha: form.pwd.clone(),
            };
        }
        return if senha_digitada {
            ResolucaoWifi::TrocarSenha {
                senha: form.pwd.clone(),
            }
        } else {
            ResolucaoWifi::Preservar
        };
    }

    if ssid_digitado {
        return ResolucaoWifi::Gravar {
            ssid: form.ssid.clone(),
            senha: form.pwd.clone(),
        };
    }
    match ssid_salvo {
        Some(_) => ResolucaoWifi::Preservar,
        None => ResolucaoWifi::SemRede,
    }
}
