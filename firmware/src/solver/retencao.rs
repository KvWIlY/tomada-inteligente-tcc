//! Retencao local: qual balde apagar quando o disco esta acabando.
//!
//! A purga por **idade** (a que ja existia) resolve o regime permanente, mas nao
//! protege a operacao: a 1 medicao/min o disco enche em ~2,5 dias e a primeira purga
//! por idade so acontece no 5o. Este modulo e o gatilho que faltava - decide, a partir
//! do espaco livre, QUAIS baldes remover.
//!
//! Regras (o "combinado" da retencao):
//!   - Remove do **mais antigo para o mais novo**, nunca o balde do dia corrente
//!     (e nele que estamos gravando agora).
//!   - **Prefere balde ja confirmado**: o dado dele ja esta no banco, entao apagar
//!     nao perde nada. So depois de esgotar os confirmados cai para os nao
//!     confirmados - ai ha perda real, e quem chama precisa registrar isso.
//!   - Para assim que o alvo de espaco livre e atingido: nao apaga mais que o preciso.
//!
//! Granularidade de balde (1 arquivo/dia) mantem a remocao O(1) por balde - um
//! `remove`, sem reescrever arquivo (Principio III: poupar a flash).
//!
//! Puro: entra o inventario + o espaco, sai o plano. Sem I/O, host-testavel.

/// Um balde de medicoes em disco, do ponto de vista da purga.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Balde {
    /// Indice do dia (epoch / 86400) - tambem a ordem de antiguidade.
    pub dia: i64,
    /// Tamanho do arquivo em bytes (o que sera liberado ao remove-lo).
    pub bytes: u64,
    /// `true` quando TODOS os registros do balde ja foram confirmados pelo servidor.
    /// Balde vazio conta como confirmado: nao ha o que perder.
    pub todos_confirmados: bool,
}

/// O que a purga por espaco decidiu fazer.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PlanoPurga {
    /// Dias a remover, na ordem em que devem ser removidos.
    pub remover: Vec<i64>,
    /// Soma dos bytes dos baldes em `remover`.
    pub bytes_liberados: u64,
    /// Quantos dos baldes em `remover` tinham registro NAO confirmado - ou seja,
    /// quantos representam perda real de dado que nunca chegou ao banco.
    pub baldes_com_perda: usize,
    /// `false` quando nem removendo tudo que era elegivel da para atingir `alvo_livre`
    /// - sinal de que a gravacao local ainda pode falhar.
    pub suficiente: bool,
}

/// Decide quais baldes remover para o espaco livre voltar a `alvo_livre`.
///
/// `livre` e `alvo_livre` em bytes; `dia_atual` = `agora / 86400` (o balde protegido).
/// Nao dispara nada enquanto `livre >= alvo_livre` - quem chama e que decide o
/// **gatilho** (tipicamente mais baixo que o alvo, para nao purgar a cada tick).
pub fn planejar_por_espaco(
    baldes: &[Balde],
    livre: u64,
    alvo_livre: u64,
    dia_atual: i64,
) -> PlanoPurga {
    let mut plano = PlanoPurga::default();

    let falta = match alvo_livre.checked_sub(livre) {
        None | Some(0) => {
            plano.suficiente = true;
            return plano;
        }
        Some(f) => f,
    };

    // Do mais antigo para o mais novo; o balde do dia corrente nunca entra.
    let mut elegiveis: Vec<&Balde> = baldes.iter().filter(|b| b.dia != dia_atual).collect();
    elegiveis.sort_unstable_by_key(|b| b.dia);

    // Duas passadas: primeiro os confirmados (perda zero), depois os nao confirmados.
    'passadas: for confirmados in [true, false] {
        for b in &elegiveis {
            if plano.bytes_liberados >= falta {
                break 'passadas;
            }
            if b.todos_confirmados != confirmados {
                continue;
            }
            plano.remover.push(b.dia);
            plano.bytes_liberados += b.bytes;
            if !b.todos_confirmados {
                plano.baldes_com_perda += 1;
            }
        }
    }

    plano.suficiente = plano.bytes_liberados >= falta;
    plano
}
