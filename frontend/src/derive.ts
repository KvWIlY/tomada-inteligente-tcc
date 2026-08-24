// =============================================
// Dados derivados das medições reais p/ as views do handoff
// =============================================

import type { Medicao, Tomada } from './types';

/** Reamostra a potência das medições para `n` pontos (p/ gráficos SVG). */
export function seriesFromMedicoes(medicoes: Medicao[], n = 48): number[] {
  if (medicoes.length === 0) return new Array(n).fill(0);
  if (medicoes.length <= n) return medicoes.map((m) => m.p_medida);
  const out: number[] = [];
  const bucket = medicoes.length / n;
  for (let i = 0; i < n; i++) {
    const start = Math.floor(i * bucket);
    const end = Math.max(start + 1, Math.floor((i + 1) * bucket));
    let sum = 0;
    for (let j = start; j < end; j++) sum += medicoes[j].p_medida;
    out.push(sum / (end - start));
  }
  return out;
}

/**
 * Consumo total agora (W): soma de p_medida_atual das tomadas não-OFF.
 *
 * Fica no cliente **de propósito**, mesmo o backend expondo `consumo_total_atual`
 * em `GET /residencias/{id}`: este número precisa refletir o estado OTIMISTA da UI
 * no instante em que o usuário aciona um relé, antes de o servidor confirmar. Usar
 * o valor do servidor aqui deixaria a projeção de P_max defasada justamente no
 * momento em que ela decide se o comando é aceito.
 *
 * O *efficiency score*, ao contrário, saiu daqui: depende do histórico de todas as
 * tomadas e é calculado pelo backend (`db::tomadas::visao_geral`), fonte única.
 */
export function consumoTotalAtual(tomadas: Tomada[]): number {
  return tomadas.reduce((s, t) => s + (t.estado === 'OFF' ? 0 : t.p_medida_atual), 0);
}

/**
 * Energia consumida na janela (kWh), integrando P ao longo do tempo REAL.
 *
 * Watt instantâneo não prova redução de consumo; energia acumulada prova. É este
 * o número que sustenta a tese do trabalho (corte de consumo fantasma), e por
 * isso ele sai dos timestamps de verdade, não de um intervalo médio presumido:
 * o buffer do ESP32 tem buracos sempre que o nó fica offline, e assumir cadência
 * constante inventaria energia que não foi medida.
 *
 * Cada amostra vale pelo intervalo que a separa da anterior (retângulo à
 * esquerda). Intervalos maiores que `capSeg` são **descartados**, não
 * extrapolados: um buraco de 6 h no meio da série significa que não se sabe o
 * que houve ali, e preencher com a última leitura seria fabricar dado.
 */
export function energiaKWh(medicoes: Medicao[], capSeg = 15 * 60): number {
  let wh = 0;
  for (let i = 1; i < medicoes.length; i++) {
    const dt = medicoes[i].timestamp - medicoes[i - 1].timestamp;
    if (dt <= 0 || dt > capSeg) continue;
    wh += (medicoes[i - 1].p_medida * dt) / 3600;
  }
  return wh / 1000;
}

/**
 * Fator de potência agregado da janela, ou `null` se nenhuma medição o trouxe.
 *
 * NÃO é a média aritmética dos FPs: fator de potência é razão entre potências, e
 * a média simples daria o mesmo peso a uma amostra de 2 W e a uma de 120 W. O
 * agregado correto é `ΣP / ΣS`, e como só guardamos P e FP, `S = P/FP`.
 *
 * Medições anteriores a 2026-08-24 têm `fator_potencia` nulo (o firmware ainda
 * gravava potência aparente e a fase nunca foi registrada). Elas são puladas —
 * tratar null como 0 afundaria o agregado com dado que simplesmente não existe.
 */
export function fatorPotenciaAgregado(medicoes: Medicao[]): number | null {
  let somaP = 0;
  let somaS = 0;
  for (const m of medicoes) {
    const fp = m.fator_potencia;
    if (fp === null || fp <= 0) continue;
    somaP += m.p_medida;
    somaS += m.p_medida / fp;
  }
  if (somaS <= 0) return null;
  return Math.min(1, somaP / somaS);
}

/** Número com 2 casas em pt-BR (vírgula decimal), como o resto do dashboard. */
export function fmt2(n: number): string {
  return n.toLocaleString('pt-BR', { minimumFractionDigits: 2, maximumFractionDigits: 2 });
}

/** True se alguma tomada tem medição nos últimos `limiteSeg` segundos. */
export function temConexaoRecente(
  tomadas: Tomada[],
  medicoesPorTomada: Record<string, Medicao[]>,
  limiteSeg = 10 * 60,
): boolean {
  const agora = Math.floor(Date.now() / 1000);
  return tomadas.some((t) => {
    const meds = medicoesPorTomada[t.id] ?? [];
    const ult = meds[meds.length - 1];
    return ult ? agora - ult.timestamp < limiteSeg : false;
  });
}
