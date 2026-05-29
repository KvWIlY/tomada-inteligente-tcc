// =============================================
// Hero -- detalhe da tomada selecionada
// =============================================
//
// Layout fiel ao mockup:
//   ESQUERDA: eyebrow (local · residencia), titulo grande, id+firmware,
//             estado badge grande
//   DIREITA: 3 KPIs (potencia atual, tempo no estado, ultima atualizacao)
//
// Botao de liga/desliga: pequeno, posicionado logo abaixo da badge
// (o mockup nao tem botao no hero, mas precisamos pra interatividade).

import { ICONS } from '../icons';
import type { EstadoAtual, Residencia, Tomada } from '../types';
import { relTime, STATE_INFO } from '../utils';

const FIRMWARE_VERSION = (import.meta.env.VITE_FIRMWARE_VERSION as string | undefined) ?? 'v0.1.0';

export interface HeroProps {
  tomada: Tomada;
  residencia: Residencia | null;
  estadoAtual: EstadoAtual;
  onToggleRele: () => void;
}

export function renderHero(el: HTMLElement, p: HeroProps): void {
  const info = STATE_INFO[p.estadoAtual.estado];

  // Tempo no estado: "há 14 min" ou "há 2h 14min"
  const m = p.estadoAtual.tempo_no_estado;
  const tempoStr = m < 60
    ? `${m} min`
    : `${Math.floor(m / 60)}h ${(m % 60).toString().padStart(2, '0')}min`;

  // ID curto (8 primeiros chars do UUID, igual o mockup com "tom_01")
  const idCurto = p.tomada.id.length > 8 ? p.tomada.id.slice(0, 8) : p.tomada.id;

  const eyebrow = p.residencia
    ? `${escapeHtml(p.tomada.localizacao ?? '').toUpperCase()} · ${escapeHtml(p.residencia.nome).toUpperCase()}`
    : escapeHtml(p.tomada.localizacao ?? '').toUpperCase();

  const tsUltima = Math.floor(Date.now() / 1000) - p.estadoAtual.ultima_atualizacao_seg;
  const atuStr = relTime(tsUltima);

  const criticalTag = p.tomada.config.is_critical
    ? `<span style="margin-left:8px;padding:2px 8px;background:var(--off-soft);color:var(--off);border-radius:6px;font-size:11px;font-weight:500">CRÍTICO</span>`
    : '';

  const releTooltip = p.tomada.rele_aberto ? 'Ligar tomada' : 'Desligar tomada';
  const releIcon = ICONS.power;

  el.innerHTML = `
    <section class="hero" data-screen-label="hero" style="--hero-state-soft:${info.soft}">
      <div class="hero-left">
        <div class="hero-eyebrow">${eyebrow}</div>
        <h1 class="hero-title">${escapeHtml(p.tomada.nome)}</h1>
        <div class="hero-loc">
          id <code style="font-size:12px;color:var(--fg-muted)">${escapeHtml(idCurto)}</code>
          · firmware ${FIRMWARE_VERSION}
          ${criticalTag}
        </div>

        <div class="state-badge"
             style="--state-color:${info.color};--state-soft:${info.soft};--state-border:${info.color}">
          <div class="state-glyph">${p.estadoAtual.estado}</div>
          <div class="state-info">
            <div class="state-label">Estado atual</div>
            <div class="state-name">${info.label}</div>
          </div>
          <button class="state-toggle" data-toggle-rele title="${releTooltip}">
            ${releIcon}
          </button>
        </div>
      </div>

      <div class="hero-right">
        <div class="kpi">
          <div class="kpi-label">Potência atual</div>
          <div class="kpi-value">
            ${p.estadoAtual.p_atual.toFixed(1)}<span class="unit">W</span>
          </div>
          <div class="kpi-sub">última leitura do sensor</div>
        </div>
        <div class="kpi">
          <div class="kpi-label">No estado</div>
          <div class="kpi-value">${tempoStr}</div>
          <div class="kpi-sub">desde a última transição</div>
        </div>
        <div class="kpi">
          <div class="kpi-label">Atualização</div>
          <div class="kpi-value" style="font-size:1.2rem">${atuStr}</div>
          <div class="kpi-sub">via Supabase Realtime</div>
        </div>
      </div>
    </section>
  `;

  el.querySelector<HTMLButtonElement>('[data-toggle-rele]')?.addEventListener('click', () => p.onToggleRele());
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}
