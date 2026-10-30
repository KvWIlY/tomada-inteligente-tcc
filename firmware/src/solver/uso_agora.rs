//! Janela de "usar agora": o dono quer o aparelho de volta AGORA, depois que o MILP
//! cortou (feature 005, religar local).
//!
//! Problema que isto resolve: quando o rele abre, a potencia medida vai a zero, entao
//! `U_i = P > limiar` nunca mais e verdadeiro e a R3 nunca mais e acionada. Sem uma
//! porta de entrada humana, a tomada so voltaria a energizar se `alpha*W_i > C_std`
//! - o que, com os pesos default, nao acontece. Numa casa em modo standalone nao ha
//! nem dashboard: a tomada ficaria cortada para sempre.
//!
//! Sao DUAS portas para a MESMA intencao (botao fisico e dashboard), por isso a regra
//! vive num lugar so. E deliberadamente TEMPORARIA: passados `DURACAO_S`, a decisao
//! volta pro MILP sozinha. "Ficar sempre ligada" (geladeira, roteador) NAO e isto -
//! e `is_critical` na config, que vira `W_i = M` e e resolvido DENTRO do modelo.
//!
//! Puro (sem I/O, sem relogio interno): o chamador passa o uptime. Testavel no host
//! (Principio IX). O tempo aqui e sempre UPTIME monotonico, nunca epoch - a janela
//! precisa funcionar numa casa sem hora certa.

/// Duracao padrao da janela (s). 30 min: tempo de o dono ligar a TV e usar de fato.
pub const DURACAO_S: i64 = 30 * 60;

/// De onde veio o pedido. Muda so a observabilidade - o efeito e identico.
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

    /// Segundos restantes (0 se ja expirou) - vai para o log/evento.
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
/// janela eterna - falhar fechado e a regra (Principio VIII).
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
    /// e SUBORDINADO ao P_max - R2 continua vencendo, inclusive negando o pedido).
    pub forcar_ligada: bool,
    /// Zera `F_i` enquanto durar. Sem isto o religar duraria um ciclo: a tomada estava
    /// ociosa ha muito mais que `t_limite`, entao a R4 proibiria STD e o solver
    /// cortaria de novo na hora seguinte - o botao pareceria nao funcionar.
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

/// `true` quando existe uma janela que JA NAO VALE - o chamador deve descarta-la,
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
///   `Some(false)` = "usar agora" - abre a janela.
///
/// `ja_consumido` evita o laco: o servidor continua devolvendo `Some(false)` ate o
/// firmware liberar o comando no proximo snapshot, e sem esta guarda cada tick de
/// nuvem reabriria a janela para sempre.
pub fn deve_abrir_do_dashboard(rele_aberto: Option<bool>, ja_consumido: bool) -> bool {
    rele_aberto == Some(false) && !ja_consumido
}
