// =============================================
// Helpers compartilhados pelas views
// =============================================

import type { Estado, EstadoAtual, Medicao, Tomada } from './types';

/**
 * Detecta o intervalo medio entre medicoes a partir dos timestamps.
 * Retorna em horas para uso direto em calculos de Wh (P_w * h = Wh).
 * Fallback de 1 min quando ha menos de 2 amostras.
 */
export function calcIntervaloHoras(medicoes: Medicao[]): number {
  if (medicoes.length < 2) return 1 / 60;
  const spanSec = medicoes[medicoes.length - 1].timestamp - medicoes[0].timestamp;
  return spanSec / (medicoes.length - 1) / 3600;
}

// Cor + label + descricao por estado (reusado em hero, picker, chart)
// Labels seguem o mockup do designer.
export const STATE_INFO: Record<Estado, { label: string; color: string; soft: string; desc: string }> = {
  ON:  { label: 'Em uso ativo',                color: 'oklch(0.68 0.16 155)', soft: 'oklch(0.68 0.16 155 / 0.14)', desc: 'Aparelho ativo agora' },
  STD: { label: 'Standby permitido',           color: 'oklch(0.78 0.13 75)',  soft: 'oklch(0.78 0.13 75 / 0.16)',  desc: 'Consumo reduzido' },
  OFF: { label: 'Relé aberto · economizando',  color: 'oklch(0.65 0.18 25)',  soft: 'oklch(0.65 0.18 25 / 0.14)',  desc: 'Sem consumo' },
};

/** Formata "HH:MM" a partir de unix epoch em segundos. */
export function fmtTime(unixSec: number): string {
  const d = new Date(unixSec * 1000);
  return d.toLocaleTimeString('pt-BR', { hour: '2-digit', minute: '2-digit' });
}

/** Tempo relativo: "agora", "5 min atras", "2h atras", "ontem". */
export function relTime(unixSec: number): string {
  const diffSec = Math.floor(Date.now() / 1000) - unixSec;
  if (diffSec < 30) return 'agora';
  if (diffSec < 60) return `${diffSec}s atras`;
  const min = Math.floor(diffSec / 60);
  if (min < 60) return `${min} min atras`;
  const h = Math.floor(min / 60);
  if (h < 24) return `${h}h atras`;
  const d = Math.floor(h / 24);
  if (d === 1) return 'ontem';
  return `${d} dias atras`;
}

/** Formata Wh com 1 casa decimal e separador. */
export function fmtWh(wh: number): string {
  if (wh >= 1000) return `${(wh / 1000).toFixed(2)} kWh`;
  return `${wh.toFixed(1)} Wh`;
}

/**
 * Calcula o estado atual a partir das medicoes da tomada.
 * - estado: ultimo medido (ou override pra demo)
 * - p_atual: ultima potencia
 * - tempo_no_estado: minutos contiguos do mesmo estado, do final pra tras
 */
export function calcEstadoAtual(
  tomada: Tomada,
  medicoes: Medicao[],
  override: Estado | null,
): EstadoAtual {
  if (medicoes.length === 0) {
    return {
      estado: override ?? tomada.estado,
      p_atual: 0,
      tempo_no_estado: 0,
      ultima_atualizacao_seg: 0,
    };
  }
  const ult = medicoes[medicoes.length - 1];
  const estado = override ?? ult.estado;
  let mins = 0;
  for (let i = medicoes.length - 1; i >= 0; i--) {
    if (medicoes[i].estado === ult.estado) mins++;
    else break;
  }
  const ultimaAtualizacaoSec = Math.max(0, Math.floor(Date.now() / 1000) - ult.timestamp);
  return {
    estado,
    p_atual: ult.p_medida,
    tempo_no_estado: mins,
    ultima_atualizacao_seg: ultimaAtualizacaoSec,
  };
}

/** Aplica tweaks no document root: theme, accent, density. */
export function aplicarTweaks(theme: string, accent: string, density: string): void {
  document.documentElement.dataset.theme = theme;
  const accentH: Record<string, number> = {
    violet: 285, indigo: 260, teal: 195, orange: 45, pink: 350,
  };
  document.documentElement.style.setProperty('--accent-h', String(accentH[accent] ?? 285));
  const dens = density === 'compact' ? '0.85' : density === 'spacious' ? '1.15' : '1';
  document.documentElement.style.setProperty('--density', dens);
}
