//! Anti-replay do heartbeat ESP-NOW sem relogio (feature 005, D6 / FR-010).
//!
//! O protocolo v4 usava `epoch` + janela de +-30 s. Isso exige SNTP nos dois nos: numa
//! casa sem internet os relogios divergem e as tomadas passam a REJEITAR heartbeats
//! legitimos umas das outras — a coordenacao morre em silencio.
//!
//! v5 troca o carimbo por `(boot_id, contador)`:
//!   - `boot_id`: 32 bits aleatorios sorteados a cada boot (nunca 0);
//!   - `contador`: incrementa a cada difusao, vive em RAM (Principio III: zero
//!     escrita em Flash).
//!
//! O receptor guarda o par por remetente. Risco residual aceito (Clarification Q2):
//! reapresentar um boot ANTIGO inteiro passa como "reinicio". O impacto e limitado ao
//! valor de consumo atribuido aquele remetente — nunca comandos, rele ou teto.

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
    /// Primeiro heartbeat deste remetente — passa a acompanhar.
    Primeiro,
    /// Sequencia normal dentro do mesmo boot.
    Aceito,
    /// `boot_id` novo: a tomada vizinha reiniciou (legitimo, sem novo pareamento).
    Reancorado,
    /// Contador nao cresceu no mesmo boot: replay ou duplicata de radio.
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
/// Devolve `(veredito, novo_estado)` — `novo_estado` so muda quando o veredito passa.
pub fn aceitar(
    estado: Option<EstadoPeer>,
    boot_id: u32,
    contador: u32,
) -> (Veredito, Option<EstadoPeer>) {
    // boot_id 0 e reservado (emissor sem gerador de aleatorios funcionando).
    if boot_id == 0 {
        return (Veredito::Rejeitado, estado);
    }
    match estado {
        None => (Veredito::Primeiro, Some(EstadoPeer::novo(boot_id, contador))),
        Some(e) if e.boot_id != boot_id => (
            Veredito::Reancorado,
            Some(EstadoPeer::novo(boot_id, contador)),
        ),
        Some(e) if contador > e.ultimo_contador => (
            Veredito::Aceito,
            Some(EstadoPeer::novo(boot_id, contador)),
        ),
        Some(e) => (Veredito::Rejeitado, Some(e)),
    }
}

/// Proximo contador a difundir. Satura em vez de dar wrap: com wrap, o contador
/// voltaria a 1 e o proprio no seria tratado como replay pelos vizinhos ate reiniciar.
/// A 1 difusao/10 s, saturar exige ~1360 anos — na pratica, nunca.
pub fn proximo_contador(atual: u32) -> u32 {
    atual.saturating_add(1)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn primeiro_heartbeat_de_um_peer_desconhecido_passa() {
        let (v, e) = aceitar(None, 0xAABB_CCDD, 1);
        assert_eq!(v, Veredito::Primeiro);
        assert!(v.passou());
        assert_eq!(e, Some(EstadoPeer::novo(0xAABB_CCDD, 1)));
    }

    #[test]
    fn sequencia_crescente_no_mesmo_boot_e_aceita() {
        let mut est = None;
        for c in 1..=5u32 {
            let (v, novo) = aceitar(est, 7, c);
            assert!(v.passou(), "contador {c} deveria passar");
            est = novo;
        }
        assert_eq!(est.unwrap().ultimo_contador, 5);
    }

    #[test]
    fn replay_do_mesmo_quadro_e_rejeitado() {
        let est = Some(EstadoPeer::novo(7, 42));
        let (v, novo) = aceitar(est, 7, 42);
        assert_eq!(v, Veredito::Rejeitado);
        assert!(!v.passou());
        assert_eq!(novo, est, "estado nao regride em rejeicao");
    }

    #[test]
    fn quadro_antigo_reapresentado_e_rejeitado() {
        let est = Some(EstadoPeer::novo(7, 100));
        let (v, novo) = aceitar(est, 7, 3);
        assert_eq!(v, Veredito::Rejeitado);
        assert_eq!(novo.unwrap().ultimo_contador, 100);
    }

    #[test]
    fn reinicio_legitimo_do_vizinho_e_reancorado_sem_novo_pareamento() {
        let est = Some(EstadoPeer::novo(7, 5000));
        // A vizinha reiniciou: boot_id novo, contador volta a 1.
        let (v, novo) = aceitar(est, 9, 1);
        assert_eq!(v, Veredito::Reancorado);
        assert!(v.passou());
        assert_eq!(novo, Some(EstadoPeer::novo(9, 1)));

        // E a sequencia segue normalmente a partir dai.
        let (v2, _) = aceitar(novo, 9, 2);
        assert_eq!(v2, Veredito::Aceito);
    }

    #[test]
    fn nao_depende_de_relogio_algum() {
        // Todo o modulo opera sem tempo: dois nos com relogios completamente
        // diferentes trocam heartbeats normalmente (FR-010).
        let mut est = None;
        for c in 1..=3 {
            let (v, novo) = aceitar(est, 0xDEAD_BEEF, c);
            assert!(v.passou());
            est = novo;
        }
    }

    #[test]
    fn boot_id_zero_e_recusado() {
        let (v, e) = aceitar(None, 0, 1);
        assert_eq!(v, Veredito::Rejeitado);
        assert_eq!(e, None);
    }

    #[test]
    fn contador_satura_em_vez_de_dar_wrap() {
        assert_eq!(proximo_contador(1), 2);
        assert_eq!(proximo_contador(u32::MAX), u32::MAX);
        // Saturado: o proximo quadro seria "nao crescente" e rejeitado — preferivel a
        // voltar para 1 e ser confundido com replay.
        let est = Some(EstadoPeer::novo(7, u32::MAX));
        assert_eq!(aceitar(est, 7, u32::MAX).0, Veredito::Rejeitado);
    }
}
