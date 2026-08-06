// =============================================
// Helpers compartilhados pelas views
// =============================================

import type { Medicao } from './types';

/**
 * Detecta o intervalo médio entre medições a partir dos timestamps.
 * Retorna em horas para uso direto em cálculos de Wh (P_w * h = Wh).
 * Fallback de 1 min quando há menos de 2 amostras.
 */
export function calcIntervaloHoras(medicoes: Medicao[]): number {
  if (medicoes.length < 2) return 1 / 60;
  const spanSec = medicoes[medicoes.length - 1].timestamp - medicoes[0].timestamp;
  return spanSec / (medicoes.length - 1) / 3600;
}

/** Tempo relativo: "agora", "5 min atrás", "2h atrás", "ontem". */
export function relTime(unixSec: number): string {
  const diffSec = Math.floor(Date.now() / 1000) - unixSec;
  if (diffSec < 30) return 'agora';
  if (diffSec < 60) return `${diffSec}s atrás`;
  const min = Math.floor(diffSec / 60);
  if (min < 60) return `${min} min atrás`;
  const h = Math.floor(min / 60);
  if (h < 24) return `${h}h atrás`;
  const d = Math.floor(h / 24);
  if (d === 1) return 'ontem';
  return `${d} dias atrás`;
}
