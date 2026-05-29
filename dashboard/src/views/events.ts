// =============================================
// Linha do tempo de eventos
// =============================================

import { ICONS } from '../icons';
import type { Evento } from '../types';
import { relTime } from '../utils';

export type EventFilter = 'all' | 'chaveamento' | 'inviabilidade';

export interface EventsProps {
  eventos: Evento[];
  filter: EventFilter;
  onChangeFilter: (f: EventFilter) => void;
}

const STYLES: Record<Evento['tipo'], { color: string; soft: string; icon: string }> = {
  chaveamento:    { color: 'var(--accent)',   soft: 'var(--accent-soft)', icon: ICONS.swap },
  inviabilidade:  { color: 'var(--std)',      soft: 'var(--std-soft)',    icon: ICONS.warn },
  cold_start:     { color: 'var(--on)',       soft: 'var(--on-soft)',     icon: ICONS.check },
  config:         { color: 'var(--fg-muted)', soft: 'var(--surface-2)',   icon: ICONS.cog },
  sync_error:     { color: 'var(--off)',      soft: 'var(--off-soft)',    icon: ICONS.cloud },
};

export function renderEvents(el: HTMLElement, p: EventsProps): void {
  const filtered = p.filter === 'all' ? p.eventos : p.eventos.filter((e) => e.tipo === p.filter);

  const tabs: Array<[EventFilter, string]> = [
    ['all',           'Tudo'],
    ['chaveamento',   'Chaveamentos'],
    ['inviabilidade', 'Avisos'],
  ];

  const list = filtered.length === 0
    ? `<div style="padding:32px;text-align:center;color:var(--fg-muted);font-size:13px">
         Nenhum evento neste filtro.
       </div>`
    : filtered.map((e) => {
        const s = STYLES[e.tipo] ?? STYLES.config;
        return `
          <div class="event" style="--ev-color:${s.color};--ev-soft:${s.soft}">
            <div class="event-icon">${s.icon}</div>
            <div class="event-body">
              <div class="event-title">${escapeHtml(e.titulo)}</div>
              <div class="event-detail">${escapeHtml(e.detalhes ?? '')}</div>
            </div>
            <div class="event-time">${relTime(e.timestamp)}</div>
          </div>
        `;
      }).join('');

  el.innerHTML = `
    <section class="section">
      <div class="section-head">
        <div>
          <h2 class="section-title">${ICONS.clock} Linha do tempo de eventos</h2>
          <p class="section-sub">Decisoes e transicoes registradas nas ultimas 24h</p>
        </div>
        <div class="tabs">
          ${tabs.map(([k, l]) => `<button class="tab ${k === p.filter ? 'active' : ''}" data-filter="${k}">${l}</button>`).join('')}
        </div>
      </div>
      <div class="events">${list}</div>
    </section>
  `;

  el.querySelectorAll<HTMLButtonElement>('[data-filter]').forEach((btn) => {
    btn.addEventListener('click', () => {
      const f = btn.dataset.filter as EventFilter;
      if (f) p.onChangeFilter(f);
    });
  });
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}
