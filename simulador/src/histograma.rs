//! Histograma de duracoes com memoria que nao cresce com a rodada.
//!
//! Guardar cada duracao de `decidir` custava 4 bytes por tomada por passo: 30 d a cada 10 s
//! com 250 tomadas passavam de 250 MB, e cada `resumo.json` ainda copiava e ordenava tudo.
//! Aqui cada valor cai num balde log-linear: exato de 0 a 63 us, e acima disso 64 baldes por
//! oitava, com o representante a no maximo 1/128 (0,8%) do valor real. O maximo e exato. Uma
//! oitava so e alocada quando algum valor cai nela, entao uma tomada cujos solves ficam entre
//! 1 e 4 ms ocupa duas oitavas.

const BITS: u32 = 6;
const SUB: usize = 1 << BITS;

#[derive(Debug, Clone, Default)]
pub struct Histograma {
    oitavas: Vec<Option<Box<[u64; SUB]>>>,
    total: u64,
    max: u32,
}

impl Histograma {
    #[inline]
    pub fn registrar(&mut self, v: u32) {
        let (o, i) = posicao(v);
        if self.oitavas.len() <= o {
            self.oitavas.resize_with(o + 1, || None);
        }
        self.oitavas[o].get_or_insert_with(|| Box::new([0; SUB]))[i] += 1;
        self.total += 1;
        self.max = self.max.max(v);
    }

    pub fn len(&self) -> u64 {
        self.total
    }

    /// Percentil por posto mais proximo, com o mesmo posto de antes (`round(q x (n-1))`).
    /// Vazio devolve 0; `q >= 1` devolve o maximo exato.
    pub fn percentil(&self, q: f64) -> u32 {
        if self.total == 0 {
            return 0;
        }
        if q >= 1.0 {
            return self.max;
        }
        let posto = (q.max(0.0) * (self.total - 1) as f64).round() as u64;
        let mut acum = 0u64;
        for (o, oitava) in self.oitavas.iter().enumerate() {
            let Some(oitava) = oitava else { continue };
            for (i, &c) in oitava.iter().enumerate() {
                acum += c;
                if acum > posto {
                    return representante(o, i).min(self.max);
                }
            }
        }
        self.max
    }

    pub fn percentis(&self, qs: &[f64]) -> Vec<u32> {
        qs.iter().map(|&q| self.percentil(q)).collect()
    }
}

/// `(oitava, balde)`. Oitava 0 guarda 0..63 um por balde; a oitava `o >= 1` cobre
/// `[64 << (o-1), 128 << (o-1))` em 64 baldes iguais.
#[inline]
fn posicao(v: u32) -> (usize, usize) {
    if (v as usize) < SUB {
        return (0, v as usize);
    }
    let e = 31 - v.leading_zeros();
    let desloc = e - BITS;
    ((desloc + 1) as usize, (v >> desloc) as usize - SUB)
}

/// Meio do balde.
fn representante(oitava: usize, balde: usize) -> u32 {
    if oitava == 0 {
        return balde as u32;
    }
    let desloc = (oitava - 1) as u32;
    let lo = ((SUB + balde) as u64) << desloc;
    let hi = (((SUB + balde + 1) as u64) << desloc) - 1;
    (lo + (hi - lo) / 2).min(u32::MAX as u64) as u32
}
