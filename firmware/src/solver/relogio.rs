//! Relogio em duas fases e correcao de carimbos por deslocamento (feature 005, D1/D3).
//!
//! O firmware NUNCA pode travar o boot esperando hora certa (FR-001). Consequencia: ha
//! um periodo em que a tomada mede sem saber que horas sao. Este modulo define o que
//! fazer com esses registros - e e puro para poder ser testado no host (Principio IX).
//!
//! Regra: enquanto nao ha hora, a medicao guarda `ts = SEM_HORA` e o `uptime_s` do
//! momento. Quando o SNTP fecha o primeiro sync, calcula-se
//! `offset = epoch_real - uptime_agora` e todo pendente vira `ts = uptime_s + offset`.

/// Sentinela de "registrado sem hora certa". `0` e seguro: epoch 0 e 1970 e nenhuma
/// medicao real cai ai; o sync so envia `ts > SEM_HORA`.
pub const SEM_HORA: i64 = 0;

/// Deslocamento entre o relogio real e o uptime local, fixado no 1o sync.
pub fn offset_do_sync(epoch_real: i64, uptime_no_sync: i64) -> i64 {
    epoch_real - uptime_no_sync
}

/// Converte uptime -> epoch usando o deslocamento do sync.
pub fn epoch_de(uptime_s: i64, offset: i64) -> i64 {
    uptime_s + offset
}

/// Um registro pendente com carimbo corrigivel.
pub trait Carimbavel {
    fn ts(&self) -> i64;
    fn uptime_s(&self) -> i64;
    fn set_ts(&mut self, ts: i64);
}

/// Resultado da correcao (vai para log/evento - Principio VII).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Correcao {
    /// Quantos pendentes ganharam carimbo real.
    pub corrigidos: usize,
    /// Quantos ja tinham carimbo (nada a fazer).
    pub intactos: usize,
}

/// Corrige todos os pendentes sem hora (FR-001a). Idempotente: rodar duas vezes com o
/// mesmo offset nao altera nada na segunda passada.
pub fn corrigir<T: Carimbavel>(pendentes: &mut [T], offset: i64) -> Correcao {
    let mut c = Correcao::default();
    for p in pendentes.iter_mut() {
        if p.ts() == SEM_HORA {
            p.set_ts(epoch_de(p.uptime_s(), offset));
            c.corrigidos += 1;
        } else {
            c.intactos += 1;
        }
    }
    c
}

/// As regras que dependem da HORA DO DIA (agendamento `A_it`) nao podem ser aplicadas
/// com relogio errado - desligariam carga na hora errada (FR-001b). Enquanto
/// `relogio_ok == false`, o ciclo alimenta o solver com estes valores neutros.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RegrasHorarias {
    /// `A_it`: agenda permite uso nesta hora.
    pub a_it: bool,
}

/// Neutraliza as regras horarias quando nao ha hora certa; passa direto quando ha.
///
/// Neutro = agenda LIBERADA (`a_it = true`): sem hora, o no decide so pelo teto de
/// potencia e pela medicao - nunca desliga por agenda.
pub fn regras_horarias(relogio_ok: bool, a_it: bool) -> RegrasHorarias {
    if relogio_ok {
        RegrasHorarias { a_it }
    } else {
        RegrasHorarias { a_it: true }
    }
}
