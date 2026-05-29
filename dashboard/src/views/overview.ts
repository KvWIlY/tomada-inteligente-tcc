// =============================================
// Visao geral -- hero + grid com card de cada tomada
// =============================================

import { ICONS } from '../icons';
import type { Medicao, Residencia, Tomada } from '../types';
import { calcIntervaloHoras, STATE_INFO } from '../utils';

export interface OverviewProps {
  residencia: Residencia | null;
  tomadas: Tomada[];
  medicoesPorTomada: Record<string, Medicao[]>;
  onOpenTomada: (id: string) => void;
}

const LIMIAR_ONLINE_S = 10 * 60; // tomada considerada online se medicao < 10min atras

export function renderOverview(el: HTMLElement, p: OverviewProps): void {
  const agora = Math.floor(Date.now() / 1000);

  // Conta tomadas com medicao recente
  const onlineCount = p.tomadas.filter((t) => {
    const meds = p.medicoesPorTomada[t.id] ?? [];
    const ult = meds[meds.length - 1];
    return ult && agora - ult.timestamp < LIMIAR_ONLINE_S;
  }).length;
  const statusOnline =
    onlineCount === p.tomadas.length ? 'todas online'
    : onlineCount === 0             ? 'sem conexao recente'
    :                                 `${onlineCount}/${p.tomadas.length} online`;

  // Agregados com intervalo detectado dos timestamps
  const consumidaTotal = p.tomadas.reduce((sum, t) => {
    const meds = p.medicoesPorTomada[t.id] ?? [];
    const ih = calcIntervaloHoras(meds);
    return sum + meds.reduce((s, m) => s + m.p_medida * ih, 0);
  }, 0);
  const economizadaTotal = p.tomadas.reduce((sum, t) => {
    const meds = p.medicoesPorTomada[t.id] ?? [];
    const ih = calcIntervaloHoras(meds);
    const minOff = meds.filter((m) => m.estado === 'OFF').length;
    return sum + minOff * t.cstd * ih;
  }, 0);

  el.innerHTML = `
    <section class="hero hero-overview">
      <div class="hero-left">
        <div class="hero-eyebrow">Residencia · ${escapeHtml(p.residencia?.nome ?? '')}</div>
        <h1 class="hero-title">Visao geral</h1>
        <div class="hero-loc">${p.tomadas.length} tomadas · ${statusOnline}</div>
      </div>
      <div class="hero-right hero-right-overview">
        <div class="kpi">
          <div class="kpi-label">Consumo total hoje</div>
          <div class="kpi-value">${consumidaTotal.toFixed(0)}<span class="unit">Wh</span></div>
          <div class="kpi-sub">soma de todas as tomadas</div>
        </div>
        <div class="kpi">
          <div class="kpi-label">Economia total</div>
          <div class="kpi-value">${economizadaTotal.toFixed(0)}<span class="unit">Wh</span></div>
          <div class="kpi-sub">consumo fantasma evitado</div>
        </div>
      </div>
    </section>

    ${renderCapacityBar(p.tomadas)}

    <section style="margin-top:calc(24px * var(--density))">
      <div class="overview-grid">
        ${p.tomadas.map((t) => renderCard(t, p.medicoesPorTomada[t.id] ?? [])).join('')}
      </div>
    </section>
  `;

  el.querySelectorAll<HTMLButtonElement>('[data-tomada-id]').forEach((btn) => {
    btn.addEventListener('click', () => {
      const id = btn.dataset.tomadaId;
      if (id) p.onOpenTomada(id);
    });
  });
}

function renderCard(t: Tomada, medicoes: Medicao[]): string {
  const ult = medicoes[medicoes.length - 1];
  const estado = ult?.estado ?? t.estado;
  const pAtual = ult?.p_medida ?? 0;
  const info = STATE_INFO[estado];
  const icone = ICONS[t.icone] || ICONS.plug;
  const ih = calcIntervaloHoras(medicoes);
  const consumida = medicoes.reduce((s, m) => s + m.p_medida * ih, 0);
  const minOff = medicoes.filter((m) => m.estado === 'OFF').length;
  const economizada = minOff * t.cstd * ih;
  const criticalTag = t.config.is_critical
    ? '<div class="tomada-critical-tag">CRITICO</div>'
    : '';

  return `
    <button class="tomada-card" data-tomada-id="${t.id}"
            style="--state-color:${info.color};--state-soft:${info.soft}">
      <div class="tomada-card-head">
        <div class="tomada-card-icon">${icone}</div>
        <div class="tomada-card-head-right">
          ${criticalTag}
          <div class="tomada-state-pill">
            <span style="background:${info.color}"></span>${estado}
          </div>
        </div>
      </div>
      <div class="tomada-card-body">
        <div class="tomada-card-name">${escapeHtml(t.nome)}</div>
        <div class="tomada-card-loc">${escapeHtml(t.localizacao ?? '')}</div>
      </div>
      <div class="tomada-card-stats">
        <div>
          <div class="kpi-label">Potencia</div>
          <div class="tomada-card-stat">${pAtual.toFixed(1)}<span class="unit">W</span></div>
        </div>
        <div>
          <div class="kpi-label">Hoje</div>
          <div class="tomada-card-stat">${consumida.toFixed(0)}<span class="unit">Wh</span></div>
        </div>
        <div>
          <div class="kpi-label">Economizado</div>
          <div class="tomada-card-stat" style="color:var(--on)">${economizada.toFixed(0)}<span class="unit">Wh</span></div>
        </div>
      </div>
    </button>
  `;
}

function renderCapacityBar(tomadas: Tomada[]): string {
  if (tomadas.length === 0) return '';
  const pMax = tomadas[0]?.config?.p_max ?? 2540;
  const totalLoad = tomadas.reduce((s, t) => s + (t.p_medida_atual ?? 0), 0);
  const pct = pMax > 0 ? Math.min(100, (totalLoad / pMax) * 100) : 0;
  const color = pct > 85 ? 'var(--off)' : pct > 60 ? '#f59e0b' : 'var(--on)';

  return `
    <section class="section" style="margin-top:calc(16px * var(--density))">
      <div class="field">
        <div class="field-row">
          <div>
            <div class="field-label">${ICONS.bolt ?? '⚡'} Capacidade do circuito (P<sub>max</sub>)</div>
            <div class="field-help">${totalLoad.toFixed(0)} W / ${pMax.toFixed(0)} W (${pct.toFixed(0)}%)</div>
          </div>
        </div>
        <div style="background:var(--surface);border-radius:6px;height:12px;margin-top:8px;overflow:hidden">
          <div style="background:${color};height:100%;width:${pct}%;transition:width .3s;border-radius:6px"></div>
        </div>
      </div>
    </section>
  `;
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}
