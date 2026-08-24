//! Fallback de coordenacao pela nuvem (feature 003).
//!
//! Quando o mesh ESP-NOW nao traz NENHUM peer vivo mas o no esta online, as
//! tomadas-irmas lidas da nuvem entram como **carga fixa** (W) subtraida do
//! P_max — caminho degradado e conservador para particao do mesh. As irmas NAO
//! viram variaveis de decisao do MILP: o snapshot de nuvem e eventualmente
//! consistente e quebraria o determinismo que o ESP-NOW garante.
//!
//! Funcao pura (sem relogio interno, sem I/O) => deterministica e host-testavel
//! (Principio VII). Contrato: specs/003-.../contracts/cloud-fallback.md.

/// Dado remoto de uma tomada-irma (mapeado do GET `buscar_todas_tomadas`).
pub struct RemoteTomada {
    pub tomada_id: String,
    pub p_medida_atual: f32,
    pub ultimo_uso_ativo_ts: i64,
}

impl RemoteTomada {
    /// Entra na soma sse nao e a propria tomada e o dado esta fresco.
    /// `agora - ts` negativo (relogio da irma adiantado) tambem conta (<= janela):
    /// comportamento conservador deliberado, coberto por teste.
    fn fresca_e_alheia(&self, proprio_id: &str, agora: i64, janela_frescor_s: i64) -> bool {
        self.tomada_id != proprio_id && agora - self.ultimo_uso_ativo_ts <= janela_frescor_s
    }
}

/// Carga (W) a reservar contra o P_max pelas irmas alcancaveis so pela nuvem:
/// soma `p_medida_atual` (clamp >= 0) das irmas FRESCAS, excluindo a propria
/// tomada. Irma fresca sse `agora - ultimo_uso_ativo_ts <= janela_frescor_s`
/// (borda INCLUI). Sem irma fresca => 0.0 (degrada para local-only).
pub fn carga_fixa_nuvem(
    proprio_id: &str,
    agora: i64,
    janela_frescor_s: i64,
    irmas: &[RemoteTomada],
) -> f32 {
    irmas
        .iter()
        .filter(|t| t.fresca_e_alheia(proprio_id, agora, janela_frescor_s))
        .map(|t| t.p_medida_atual.max(0.0))
        .sum()
}

/// Quantas irmas frescas entraram na soma (observabilidade: detalhe do evento
/// `coordenacao_nuvem`, Principio VII). Mesmo criterio de `carga_fixa_nuvem`.
pub fn contar_frescas(
    proprio_id: &str,
    agora: i64,
    janela_frescor_s: i64,
    irmas: &[RemoteTomada],
) -> usize {
    irmas
        .iter()
        .filter(|t| t.fresca_e_alheia(proprio_id, agora, janela_frescor_s))
        .count()
}
