//! Anti-replay do heartbeat ESP-NOW sem relogio (feature 005, D6 / FR-010).
//!
//! O protocolo v4 usava `epoch` + janela de +-30 s. Isso exige SNTP nos dois nos: numa
//! casa sem internet os relogios divergem e as tomadas passam a REJEITAR heartbeats
//! legitimos umas das outras - a coordenacao morre em silencio.
//!
//! v5 trocou o carimbo por `(boot_id, contador)`:
//!   - `boot_id`: numero do boot do remetente (nunca 0);
//!   - `contador`: incrementa a cada difusao, vive em RAM.
//!
//! No v5/v6 o `boot_id` era aleatorio e QUALQUER `boot_id` diferente passava como
//! reinicio: um boot antigo gravado inteiro era aceito de novo (revisao do orientador,
//! 2026-10-26). No v7 o `boot_id` e o do boot anterior + 1, persistido na NVS do emissor
//! (uma escrita por boot), e o receptor so aceita um par maior que o ultimo: boot maior,
//! ou o mesmo boot com contador maior.
//!
//! Risco residual: o estado do receptor vive em RAM e o remetente e esquecido depois de
//! `TTL_PEER_S * 10` sem heartbeat aceito. So entao um boot antigo volta a passar, como
//! `Primeiro`, e so ate o remetente legitimo falar de novo com um boot maior. O mesmo
//! esquecimento readmite um emissor cuja NVS foi apagada (numero de boot voltou a 1).

/// Estado que o receptor mantem por remetente (chaveado pelo MAC).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EstadoPeer {
    pub boot_id: u32,
    pub ultimo_contador: u32,
}

impl EstadoPeer {
    pub fn novo(boot_id: u32, contador: u32) -> Self {
        EstadoPeer {
            boot_id,
            ultimo_contador: contador,
        }
    }
}

/// Decisao sobre um heartbeat recem-autenticado (HMAC ja conferido pelo chamador).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Veredito {
    /// Primeiro heartbeat deste remetente - passa a acompanhar.
    Primeiro,
    /// Sequencia normal dentro do mesmo boot.
    Aceito,
    /// `boot_id` maior: a tomada vizinha reiniciou (legitimo, sem novo pareamento).
    Reancorado,
    /// Boot menor, ou contador que nao cresceu no mesmo boot: replay ou duplicata.
    Rejeitado,
}

impl Veredito {
    /// Se o heartbeat deve ser usado na coordenacao.
    pub fn passou(self) -> bool {
        !matches!(self, Veredito::Rejeitado)
    }
}

/// Aplica a regra de frescor e ATUALIZA o estado do peer quando aceita.
///
/// `estado`: `None` = remetente ainda desconhecido.
/// Devolve `(veredito, novo_estado)` - `novo_estado` so muda quando o veredito passa.
pub fn aceitar(
    estado: Option<EstadoPeer>,
    boot_id: u32,
    contador: u32,
) -> (Veredito, Option<EstadoPeer>) {
    // boot_id 0 e reservado (o primeiro boot ja e 1).
    if boot_id == 0 {
        return (Veredito::Rejeitado, estado);
    }
    match estado {
        None => (Veredito::Primeiro, Some(EstadoPeer::novo(boot_id, contador))),
        Some(e) if boot_id > e.boot_id => (
            Veredito::Reancorado,
            Some(EstadoPeer::novo(boot_id, contador)),
        ),
        Some(e) if boot_id == e.boot_id && contador > e.ultimo_contador => (
            Veredito::Aceito,
            Some(EstadoPeer::novo(boot_id, contador)),
        ),
        Some(e) => (Veredito::Rejeitado, Some(e)),
    }
}

/// Proximo contador a difundir. Satura em vez de dar wrap: com wrap, o contador
/// voltaria a 1 e o proprio no seria tratado como replay pelos vizinhos ate reiniciar.
/// A 1 difusao/10 s, saturar exige ~1360 anos - na pratica, nunca.
pub fn proximo_contador(atual: u32) -> u32 {
    atual.saturating_add(1)
}
