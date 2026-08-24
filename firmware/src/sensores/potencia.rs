//! Calculo de potencia a partir das amostras da janela (logica pura, sem hardware).
//!
//! As amostras chegam em **mV no ADC**; a remocao do offset DC (bias do ZMPT/divisor)
//! e feita aqui (media da janela), tanto no RMS quanto no produto instantaneo.
//!
//! Duas potencias saem daqui e NAO sao a mesma coisa:
//!   * APARENTE `S = Vrms * Irms` (VA) — o que este modulo calculava ate 2026-08-24;
//!   * ATIVA `P = media(v_k * i_k)` (W) — o watt de verdade, o que o modelo consome.
//! Elas so coincidem em carga puramente resistiva. As cargas-alvo desta tomada
//! (fontes chaveadas em standby: carregador, TV, roteador) tem fator de potencia
//! baixo, e usar a aparente como se fosse watt superestimava `cstd`/`con` e apertava
//! o teto `p_max` (R2) em VA — inflando a economia de fantasma que o sistema mede.

/// Resultado de uma janela de medicao.
#[derive(Debug, Clone, Copy)]
pub struct Leitura {
    /// Tensao RMS da rede (V).
    pub vrms: f32,
    /// Corrente RMS (A).
    pub irms: f32,
    /// Potencia **ATIVA** (W). E o `p_medida` que atravessa o sistema inteiro:
    /// decisao do MILP, calibracao de `cstd`/`con`, heartbeat ESP-NOW e banco.
    pub p: f32,
    /// Potencia **APARENTE** (VA) = `vrms * irms`. Nao e persistida — existe para
    /// derivar o `fp` e para o log de diagnostico.
    pub s: f32,
    /// Fator de potencia `P/S` em [0, 1].
    pub fp: f32,
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

/// Potencia **ATIVA** (W) da janela: media do produto instantaneo das componentes AC.
///
/// `P = (1/N) * SUM_k (v_k - v_dc) * (i_k - i_dc) * fator_v * fator_i`
///
/// Centrar ANTES de multiplicar nao e detalhe: os dois canais tem bias DC de ~1,6 V e
/// o produto dos bias (~2,5e6 mV^2) enterraria o sinal util em duas ordens de grandeza.
///
/// So e valido porque `sensores::medir` amostra V e I **interleaved** (uma leitura de
/// cada por iteracao do mesmo `for`): `v_k * i_k` exige os dois no mesmo instante. Se
/// a aquisicao virar dois lacos separados, esta funcao passa a devolver ruido.
///
/// `abs()` no fim: a orientacao do SCT-013 ao redor do cabo e arbitraria na instalacao
/// e inverte o sinal do produto. Carga de tomada e passiva (nunca injeta energia), entao
/// o modulo E o consumo; sem isso um clamp montado ao contrario reportaria potencia
/// negativa e o backend recusaria o lote inteiro (`p_medida >= 0`).
pub fn potencia_ativa(v_mv: &[f32], i_mv: &[f32], fator_v: f32, fator_i: f32) -> f32 {
    let n = v_mv.len().min(i_mv.len());
    if n == 0 {
        return 0.0;
    }
    let v_dc = media(&v_mv[..n]);
    let i_dc = media(&i_mv[..n]);
    let soma: f32 = (0..n).map(|k| (v_mv[k] - v_dc) * (i_mv[k] - i_dc)).sum();
    ((soma / n as f32) * fator_v * fator_i).abs()
}

/// Fator de potencia `P/S`, saturado em [0, 1].
///
/// Sem carga (`s ~ 0`) devolve 0 — nao ha fase definida para relatar, e 0/0 viraria NaN
/// que o backend recusa. O `clamp` cobre o caso de `p` sair marginalmente acima de `s`
/// por ruido de arredondamento em corrente baixa.
pub fn fator_potencia(p_ativa: f32, p_aparente: f32) -> f32 {
    if p_aparente <= 0.0 || !p_aparente.is_finite() {
        return 0.0;
    }
    (p_ativa / p_aparente).clamp(0.0, 1.0)
}

/// Monta a `Leitura` a partir dos RMS ja convertidos (V e A) e da potencia ATIVA (W).
pub fn leitura(vrms: f32, irms: f32, p_ativa: f32) -> Leitura {
    let s = vrms * irms;
    Leitura { vrms, irms, p: p_ativa, s, fp: fator_potencia(p_ativa, s) }
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
