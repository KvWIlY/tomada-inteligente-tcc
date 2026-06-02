//! Calculo de potencia a partir de amostras RMS (logica pura, sem hardware).
//!
//! `P = Vrms * Irms` (potencia aparente — suficiente p/ o modelo; fator de potencia
//! fora de escopo, research §3). As amostras chegam em **mV no ADC**; a remocao do
//! offset DC (bias do ZMPT/divisor) e feita aqui (RMS centrado na media).

/// Resultado de uma janela de medicao.
#[derive(Debug, Clone, Copy)]
pub struct Leitura {
    /// Tensao RMS da rede (V).
    pub vrms: f32,
    /// Corrente RMS (A).
    pub irms: f32,
    /// Potencia aparente (W) = Vrms * Irms.
    pub p: f32,
}

/// Media aritmetica das amostras.
pub fn media(xs: &[f32]) -> f32 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.iter().sum::<f32>() / xs.len() as f32
}

/// RMS **centrado** (remove o offset DC): sqrt(mean((x - mean)^2)).
/// Para sinais AC com bias (ZMPT, SCT com divisor), isola a componente alternada.
pub fn rms_centrado(xs: &[f32]) -> f32 {
    if xs.is_empty() {
        return 0.0;
    }
    let m = media(xs);
    let soma_sq: f32 = xs.iter().map(|x| (x - m) * (x - m)).sum();
    (soma_sq / xs.len() as f32).sqrt()
}

/// Monta a `Leitura` a partir dos RMS ja convertidos para grandezas reais (V e A).
pub fn leitura(vrms: f32, irms: f32) -> Leitura {
    Leitura { vrms, irms, p: vrms * irms }
}

/// `U_i` (uso ativo): potencia atual acima do limiar detectado (R3).
pub fn deriva_u_i(p_atual: f32, p_limiar: f32) -> bool {
    p_atual > p_limiar
}

/// `F_i` (consumo fantasma): energizado (rele fechado) e ocioso por tempo > `t_limite`.
/// Ocioso = potencia entre o standby e o limiar de uso. `ociosa_desde_s` = ha quantos
/// segundos a tomada esta ociosa (gerido pelo ciclo); `t_limite` em s (R4).
pub fn deriva_f_i(rele_fechado: bool, p_atual: f32, p_limiar: f32, ociosa_ha_s: i64, t_limite: i64) -> bool {
    rele_fechado && p_atual <= p_limiar && ociosa_ha_s >= t_limite
}
