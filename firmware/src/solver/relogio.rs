//! Relogio em duas fases e correcao de carimbos por deslocamento (feature 005, D1/D3).
//!
//! O firmware NUNCA pode travar o boot esperando hora certa (FR-001). Consequencia: ha
//! um periodo em que a tomada mede sem saber que horas sao. Este modulo define o que
//! fazer com esses registros — e e puro para poder ser testado no host (Principio IX).
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

/// Resultado da correcao (vai para log/evento — Principio VII).
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

/// Filtro de carga do buffer persistido: pendentes de um boot ANTERIOR que nunca teve
/// hora certa sao incorrigiveis (o `uptime_s` deles se refere a outro boot) e DEVEM ser
/// descartados — excecao unica e documentada do FR-001a. Devolve quantos foram jogados
/// fora, para o evento `relogio_descartes`.
// Sem chamador no firmware: era API do crate `tomada-solver`, usada pelos testes de host.
#[allow(dead_code)]
pub fn descartar_incorrigiveis<T: Carimbavel>(pendentes: &mut Vec<T>) -> usize {
    let antes = pendentes.len();
    pendentes.retain(|p| p.ts() != SEM_HORA);
    antes - pendentes.len()
}

/// As regras que dependem da HORA DO DIA (agendamento `A_it` e historico `Pi_it`) nao
/// podem ser aplicadas com relogio errado — desligariam carga na hora errada (FR-001b).
/// Enquanto `relogio_ok == false`, o ciclo alimenta o solver com estes valores neutros.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RegrasHorarias {
    /// `A_it`: agenda permite uso nesta hora.
    pub a_it: bool,
    /// `Pi_it`: probabilidade historica de uso nesta hora.
    pub pi: f32,
}

/// Neutraliza as regras horarias quando nao ha hora certa; passa direto quando ha.
///
/// Neutro = agenda LIBERADA (`a_it = true`) e historico 0.5 (indiferente): sem hora, o
/// no decide so pelo teto de potencia e pela medicao — nunca desliga por agenda.
pub fn regras_horarias(relogio_ok: bool, a_it: bool, pi: f32) -> RegrasHorarias {
    if relogio_ok {
        RegrasHorarias { a_it, pi }
    } else {
        RegrasHorarias { a_it: true, pi: 0.5 }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    struct Med {
        ts: i64,
        uptime_s: i64,
    }
    impl Carimbavel for Med {
        fn ts(&self) -> i64 {
            self.ts
        }
        fn uptime_s(&self) -> i64 {
            self.uptime_s
        }
        fn set_ts(&mut self, ts: i64) {
            self.ts = ts;
        }
    }

    #[test]
    fn offset_reconstroi_o_epoch_do_momento_do_sync() {
        // Tomada ligou, rodou 120 s sem hora, e o SNTP fechou em 1_800_000_000.
        let offset = offset_do_sync(1_800_000_000, 120);
        assert_eq!(epoch_de(120, offset), 1_800_000_000);
        // Uma medicao feita aos 60 s de uptime aconteceu 60 s ANTES do sync.
        assert_eq!(epoch_de(60, offset), 1_800_000_000 - 60);
    }

    #[test]
    fn corrige_pendentes_sem_hora_preservando_a_ordem_temporal() {
        let mut pend = vec![
            Med { ts: SEM_HORA, uptime_s: 60 },
            Med { ts: SEM_HORA, uptime_s: 120 },
            Med { ts: SEM_HORA, uptime_s: 180 },
        ];
        let offset = offset_do_sync(1_800_000_000, 180);
        let c = corrigir(&mut pend, offset);

        assert_eq!(c.corrigidos, 3);
        assert_eq!(c.intactos, 0);
        assert_eq!(pend[0].ts, 1_800_000_000 - 120);
        assert_eq!(pend[1].ts, 1_800_000_000 - 60);
        assert_eq!(pend[2].ts, 1_800_000_000);
        assert!(pend[0].ts < pend[1].ts && pend[1].ts < pend[2].ts);
    }

    #[test]
    fn nao_mexe_em_quem_ja_tem_carimbo_e_e_idempotente() {
        let mut pend = vec![
            Med { ts: 1_700_000_000, uptime_s: 10 },
            Med { ts: SEM_HORA, uptime_s: 20 },
        ];
        let offset = offset_do_sync(1_800_000_000, 20);

        let c1 = corrigir(&mut pend, offset);
        assert_eq!((c1.corrigidos, c1.intactos), (1, 1));
        let depois = pend.clone();

        let c2 = corrigir(&mut pend, offset);
        assert_eq!((c2.corrigidos, c2.intactos), (0, 2));
        assert_eq!(pend, depois, "segunda passada nao altera nada");
    }

    #[test]
    fn nenhuma_medicao_corrigida_fica_com_horario_ficticio() {
        let mut pend = vec![Med { ts: SEM_HORA, uptime_s: 5 }];
        corrigir(&mut pend, offset_do_sync(1_800_000_000, 300));
        // Nada pode sair com ts <= SEM_HORA (o sync so envia ts > SEM_HORA).
        assert!(pend.iter().all(|m| m.ts > SEM_HORA));
    }

    #[test]
    fn descarta_apenas_os_incorrigiveis_da_carga() {
        // Excecao do FR-001a: sobraram do boot anterior sem hora => incorrigiveis.
        let mut pend = vec![
            Med { ts: 1_700_000_000, uptime_s: 10 },
            Med { ts: SEM_HORA, uptime_s: 20 },
            Med { ts: SEM_HORA, uptime_s: 30 },
        ];
        let n = descartar_incorrigiveis(&mut pend);
        assert_eq!(n, 2);
        assert_eq!(pend.len(), 1);
        assert_eq!(pend[0].ts, 1_700_000_000);
    }

    #[test]
    fn regras_horarias_ficam_neutras_sem_relogio() {
        // Com hora certa: passa o que veio da config.
        let r = regras_horarias(true, false, 0.9);
        assert_eq!(r.a_it, false);
        assert_eq!(r.pi, 0.9);

        // Sem hora certa: agenda liberada e historico indiferente (FR-001b) — o no
        // nunca desliga por agenda com hora errada.
        let r = regras_horarias(false, false, 0.9);
        assert_eq!(r.a_it, true);
        assert_eq!(r.pi, 0.5);
    }
}
