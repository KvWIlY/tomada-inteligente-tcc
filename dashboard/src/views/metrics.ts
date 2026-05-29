// =============================================
// 4 metric cards (consumo, economia, chaveamentos, tempo OFF)
// =============================================

import { ICONS } from '../icons';
import type { Medicao } from '../types';
import { calcIntervaloHoras } from '../utils';

export interface MetricsProps {
  medicoes: Medicao[];
  cstd: number;        // consumo standby calibrado da tomada (W)
  chaveamentos: number;
}

interface MetricaCalc {
  consumida: number;           // Wh
  consumidaDelta: number | null; // % tendencia (1a metade vs 2a metade das 24h), null se sem dados
  economizada: number;         // Wh
  economizadaDelta: null;
  chaveamentos: number;
  chaveamentosDelta: null;
  tempoOff: string;            // "Xh YYmin"
  tempoOffPct: number;
}

/** Calcula metricas a partir das medicoes da janela 24h. */
function calcMetricas(medicoes: Medicao[], cstd: number, chaveamentos: number): MetricaCalc {
  const ih = calcIntervaloHoras(medicoes); // horas por medicao (detectado dos timestamps)
  const totalWh = medicoes.reduce((s, m) => s + m.p_medida * ih, 0);
  const minOff = medicoes.filter((m) => m.estado === 'OFF').length;
  const economizada = minOff * cstd * ih;
  const horasOff = Math.floor(minOff * ih);
  const minsOff = Math.round((minOff * ih - horasOff) * 60);

  // Tendencia real: compara primeira metade das 24h com a segunda metade
  const mid = Math.floor(medicoes.length / 2);
  const whPrimeira = medicoes.slice(0, mid).reduce((s, m) => s + m.p_medida, 0);
  const whSegunda  = medicoes.slice(mid).reduce((s, m) => s + m.p_medida, 0);
  const consumidaDelta =
    whPrimeira > 0 ? Math.round(((whSegunda - whPrimeira) / whPrimeira) * 100) : null;

  return {
    consumida: totalWh,
    consumidaDelta,
    economizada,
    economizadaDelta: null,
    chaveamentos,
    chaveamentosDelta: null,
    tempoOff: `${horasOff}h ${minsOff.toString().padStart(2, '0')}min`,
    tempoOffPct: medicoes.length > 0 ? Math.round((minOff / medicoes.length) * 100) : 0,
  };
}

function metricCard(
  label: string,
  value: string,
  unit: string | null,
  icon: string,
  delta: number | null,
  deltaLabel: string,
  highlight = false,
  invertDelta = false,
): string {
  const isUp = delta !== null && delta > 0;
  const deltaClass = delta !== null ? ((isUp !== invertDelta) ? 'up' : 'down') : '';
  const deltaHtml =
    delta !== null
      ? `<span class="delta ${deltaClass}">${isUp ? ICONS.arrowUp : ICONS.arrowDown}${Math.abs(delta)}%</span>`
      : '';
  return `
    <div class="metric-card${highlight ? ' highlight' : ''}">
      <div class="metric-head">
        <div class="metric-label">${label}</div>
        <div class="metric-icon">${icon}</div>
      </div>
      <div class="metric-value">${value}${unit ? `<span class="unit">${unit}</span>` : ''}</div>
      <div class="metric-foot">
        ${deltaHtml}
        <span>${deltaLabel}</span>
      </div>
    </div>
  `;
}

export function renderMetrics(el: HTMLElement, p: MetricsProps): void {
  const m = calcMetricas(p.medicoes, p.cstd, p.chaveamentos);
  el.innerHTML = `
    <section class="metrics">
      ${metricCard('Energia consumida hoje', m.consumida.toFixed(0), 'Wh', ICONS.bolt, m.consumidaDelta, 'tendencia 24h', false, true)}
      ${metricCard('Economia estimada', m.economizada.toFixed(0), 'Wh', ICONS.leaf, m.economizadaDelta, 'se rele estivesse fechado', true)}
      ${metricCard('Chaveamentos hoje', String(m.chaveamentos), null, ICONS.swap, m.chaveamentosDelta, 'aberturas + fechamentos')}
      ${metricCard('Tempo fora de fantasma', m.tempoOff, null, ICONS.clock, null, `${m.tempoOffPct}% do dia em OFF`)}
    </section>
  `;
}
