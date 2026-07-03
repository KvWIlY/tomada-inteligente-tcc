// =============================================
// Dados derivados das medições reais p/ as views do handoff
// =============================================

import { calcIntervaloHoras } from './utils';
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

/** Consumo total agora (W): soma de p_medida_atual das tomadas não-OFF. */
export function consumoTotalAtual(tomadas: Tomada[]): number {
  return tomadas.reduce((s, t) => s + (t.estado === 'OFF' ? 0 : t.p_medida_atual), 0);
}

/**
 * Pontuação de eficiência (0–100): de toda a energia fantasma que poderia ter
 * sido gasta enquanto a tomada estava ociosa (não-ON), quanto % foi evitada
 * abrindo o relé (estado OFF). Retorna `null` quando não há standby calibrado em
 * nenhuma tomada (sem base para o cálculo) — a UI mostra "—" em vez de inventar um número.
 */
export function efficiencyScore(
  tomadas: Tomada[],
  medicoesPorTomada: Record<string, Medicao[]>,
): number | null {
  let evitada = 0;
  let potencial = 0;
  for (const t of tomadas) {
    if (t.cstd <= 0) continue;
    const meds = medicoesPorTomada[t.id] ?? [];
    const ih = calcIntervaloHoras(meds);
    const minOff = meds.filter((m) => m.estado === 'OFF').length;
    const minIdle = meds.filter((m) => m.estado !== 'ON').length;
    evitada += minOff * t.cstd * ih;
    potencial += minIdle * t.cstd * ih;
  }
  if (potencial <= 0) return null;
  return Math.round((evitada / potencial) * 100);
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
