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
