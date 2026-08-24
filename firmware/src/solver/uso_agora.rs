//! Janela de "usar agora": o dono quer o aparelho de volta AGORA, depois que o MILP
//! cortou (feature 005, religar local).
//!
//! Problema que isto resolve: quando o rele abre, a potencia medida vai a zero, entao
//! `U_i = P > limiar` nunca mais e verdadeiro e a R3 nunca mais e acionada. Sem uma
//! porta de entrada humana, a tomada so voltaria a energizar se `alpha*Pi*W_i > C_std`
//! — o que, com os pesos default, nao acontece. Numa casa em modo standalone nao ha
//! nem dashboard: a tomada ficaria cortada para sempre.
//!
//! Sao DUAS portas para a MESMA intencao (botao fisico e dashboard), por isso a regra
//! vive num lugar so. E deliberadamente TEMPORARIA: passados `DURACAO_S`, a decisao
//! volta pro MILP sozinha. "Ficar sempre ligada" (geladeira, roteador) NAO e isto —
//! e `is_critical` na config, que vira `W_i = M` e e resolvido DENTRO do modelo.
//!
//! Puro (sem I/O, sem relogio interno): o chamador passa o uptime. Testavel no host
//! (Principio IX). O tempo aqui e sempre UPTIME monotonico, nunca epoch — a janela
//! precisa funcionar numa casa sem hora certa.

/// Duracao padrao da janela (s). 30 min: tempo de o dono ligar a TV e usar de fato.
pub const DURACAO_S: i64 = 30 * 60;

/// De onde veio o pedido. Muda so a observabilidade — o efeito e identico.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origem {
    /// Botao fisico da propria tomada (unica porta no modo standalone).
    Botao,
    /// Botao "usar agora" do dashboard, entregue via `GET /config`.
    Dashboard,
}

impl Origem {
    pub fn as_str(self) -> &'static str {
        match self {
            Origem::Botao => "botao",
            Origem::Dashboard => "dashboard",
        }
    }
}

/// Uma janela aberta. `Copy` de proposito: o ciclo carrega isto num `Option` simples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Janela {
    pub origem: Origem,
    /// Uptime em que foi aberta.
    pub aberta_em: i64,
    /// Uptime em que deixa de valer (exclusivo: `uptime >= expira_em` ja expirou).
    pub expira_em: i64,
}

impl Janela {
    /// Ainda vale neste instante?
    pub fn ativa(&self, uptime_agora: i64) -> bool {
        uptime_agora < self.expira_em
    }

    /// Segundos restantes (0 se ja expirou) — vai para o log/evento.
    pub fn restante_s(&self, uptime_agora: i64) -> i64 {
        (self.expira_em - uptime_agora).max(0)
    }
}

/// Abre uma janela com a duracao padrao.
pub fn abrir(uptime_agora: i64, origem: Origem) -> Janela {
    abrir_por(uptime_agora, origem, DURACAO_S)
}

/// Abre uma janela com duracao explicita (usado pelos testes e por uma eventual
/// duracao configuravel). Duracao <= 0 produz uma janela ja expirada, nunca uma
/// janela eterna — falhar fechado e a regra (Principio VIII).
pub fn abrir_por(uptime_agora: i64, origem: Origem, duracao_s: i64) -> Janela {
    Janela {
        origem,
        aberta_em: uptime_agora,
        expira_em: uptime_agora + duracao_s.max(0),
    }
}

/// O que a janela faz com o ciclo de decisao neste instante.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Efeito {
    /// Fecha o rele (entregue ao ciclo como o comando manual "manter ligada", que ja
    /// e SUBORDINADO ao P_max — R2 continua vencendo, inclusive negando o pedido).
    pub forcar_ligada: bool,
    /// Zera `F_i` enquanto durar. Sem isto o religar duraria um ciclo: a tomada estava
    /// ociosa ha muito mais que `t_limite`, entao a R4 proibiria STD e o solver
    /// cortaria de novo na hora seguinte — o botao pareceria nao funcionar.
    pub suprimir_fantasma: bool,
}

/// Resolve a precedencia entre o corte manual e a janela.
///
/// `cortar_manual` = o dono mandou DESLIGAR pelo dashboard (`rele_aberto = true`).
/// Esse comando vence uma janela aberta: e a intencao humana mais recente e explicita,
/// e desligar nunca e inseguro.
pub fn efeito(janela: Option<Janela>, cortar_manual: bool, uptime_agora: i64) -> Efeito {
    if cortar_manual {
        return Efeito::default();
    }
    match janela {
        Some(j) if j.ativa(uptime_agora) => Efeito {
            forcar_ligada: true,
            suprimir_fantasma: true,
        },
        _ => Efeito::default(),
    }
}

/// `true` quando existe uma janela que JA NAO VALE — o chamador deve descarta-la,
/// registrar o evento e (no modo nuvem) liberar o comando manual no servidor.
/// Separado de `efeito` porque e uma TRANSICAO: acontece uma vez, nao todo ciclo.
pub fn expirou(janela: Option<Janela>, uptime_agora: i64) -> bool {
    matches!(janela, Some(j) if !j.ativa(uptime_agora))
}

/// O `rele_aberto` do `GET /config` pede uma janela nova?
///
/// Contrato do campo (nulavel a partir da migration 0002):
///   `None`        = automatico, o MILP manda;
///   `Some(true)`  = cortar (trava explicita do dono, permanece ate ele desfazer);
///   `Some(false)` = "usar agora" — abre a janela.
///
/// `ja_consumido` evita o laco: o servidor continua devolvendo `Some(false)` ate o
/// firmware liberar o comando no proximo snapshot, e sem esta guarda cada tick de
/// nuvem reabriria a janela para sempre.
pub fn deve_abrir_do_dashboard(rele_aberto: Option<bool>, ja_consumido: bool) -> bool {
    rele_aberto == Some(false) && !ja_consumido
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn abrir_marca_o_fim_pela_duracao_padrao() {
        let j = abrir(1_000, Origem::Botao);
        assert_eq!(j.aberta_em, 1_000);
        assert_eq!(j.expira_em, 1_000 + DURACAO_S);
        assert_eq!(j.origem, Origem::Botao);
    }

    #[test]
    fn ativa_dentro_expirada_na_borda_e_depois() {
        let j = abrir_por(100, Origem::Botao, 60);
        assert!(j.ativa(100), "abre valendo no mesmo instante");
        assert!(j.ativa(159));
        assert!(!j.ativa(160), "borda e exclusiva");
        assert!(!j.ativa(9_999));
    }

    #[test]
    fn duracao_nao_positiva_nasce_expirada() {
        // Falha fechado: uma config zoada nunca deve produzir tomada ligada p/ sempre.
        assert!(!abrir_por(100, Origem::Botao, 0).ativa(100));
        assert!(!abrir_por(100, Origem::Botao, -50).ativa(100));
    }

    #[test]
    fn restante_nunca_e_negativo() {
        let j = abrir_por(100, Origem::Dashboard, 60);
        assert_eq!(j.restante_s(100), 60);
        assert_eq!(j.restante_s(130), 30);
        assert_eq!(j.restante_s(160), 0);
        assert_eq!(j.restante_s(5_000), 0);
    }

    #[test]
    fn sem_janela_nao_ha_efeito() {
        assert_eq!(efeito(None, false, 100), Efeito::default());
    }

    #[test]
    fn janela_ativa_liga_e_suprime_fantasma() {
        let j = Some(abrir_por(100, Origem::Botao, 60));
        let e = efeito(j, false, 130);
        assert!(e.forcar_ligada);
        assert!(e.suprimir_fantasma, "sem isto a R4 cortaria de novo em t_limite");
    }

    #[test]
    fn janela_expirada_devolve_a_decisao_ao_milp() {
        let j = Some(abrir_por(100, Origem::Botao, 60));
        assert_eq!(efeito(j, false, 160), Efeito::default());
    }

    #[test]
    fn corte_manual_vence_janela_aberta() {
        // O dono apertou o botao e depois mandou desligar pelo dashboard: a ordem
        // mais recente e explicita ganha, e desligar nunca viola o P_max.
        let j = Some(abrir_por(100, Origem::Botao, 600));
        assert_eq!(efeito(j, true, 200), Efeito::default());
    }

    #[test]
    fn expirou_so_na_transicao_e_nunca_sem_janela() {
        let j = Some(abrir_por(100, Origem::Dashboard, 60));
        assert!(!expirou(None, 500), "sem janela nao ha transicao");
        assert!(!expirou(j, 130), "ainda valendo");
        assert!(expirou(j, 160));
    }

    #[test]
    fn dashboard_abre_uma_vez_por_comando() {
        assert!(deve_abrir_do_dashboard(Some(false), false));
        assert!(
            !deve_abrir_do_dashboard(Some(false), true),
            "o servidor repete o comando ate ser liberado; nao pode reabrir em laco"
        );
        assert!(!deve_abrir_do_dashboard(Some(true), false), "true = cortar");
        assert!(!deve_abrir_do_dashboard(None, false), "None = automatico");
    }

    #[test]
    fn origem_rotulada_para_o_evento() {
        assert_eq!(Origem::Botao.as_str(), "botao");
        assert_eq!(Origem::Dashboard.as_str(), "dashboard");
    }
}
