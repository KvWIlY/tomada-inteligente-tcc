// =============================================
// Sidebar de tomadas (PlugPicker)
// =============================================

import { ICONS } from '../icons';
import type { Medicao, Tomada } from '../types';
import { relTime, STATE_INFO } from '../utils';

export interface PickerProps {
  tomadas: Tomada[];
  medicoesPorTomada: Record<string, Medicao[]>;
  selectedId: string | null;
  showAll: boolean;
  onSelect: (id: string) => void;
  onShowAll: () => void;
}

let collapsed = false;

function toggleCollapsed(el: HTMLElement): void {
  collapsed = !collapsed;
  document.querySelector('.app-with-picker')?.classList.toggle('picker-collapsed', collapsed);
  renderPicker(el, (el as any).__pickerProps);
}

export function renderPicker(el: HTMLElement, p: PickerProps): void {
  (el as any).__pickerProps = p;
  el.innerHTML = `
    <div class="picker-head">
      <div class="picker-title">Tomadas</div>
      <span class="picker-count">${p.tomadas.length}</span>
    </div>
    <div class="picker-list">
      <button class="picker-item picker-all ${p.showAll ? 'active' : ''}" data-all>
        <div class="picker-icon">${ICONS.bolt}</div>
        <div class="picker-info">
          <div class="picker-name">Visao geral</div>
          <div class="picker-sub">Todas as ${p.tomadas.length} tomadas</div>
        </div>
      </button>
      ${p.tomadas
        .map((t) => {
          const meds = p.medicoesPorTomada[t.id] ?? [];
          const ult = meds[meds.length - 1];
          const estado = ult?.estado ?? t.estado;
          const pAtual = ult?.p_medida ?? 0;
          const info = STATE_INFO[estado];
          const isActive = !p.showAll && p.selectedId === t.id;
          const icone = ICONS[t.icone] || ICONS.plug;
          const agora = Math.floor(Date.now() / 1000);
          const isOnline = ult && agora - ult.timestamp < 10 * 60;
          const sub = isOnline
            ? `${pAtual.toFixed(1)} W · ${relTime(ult!.timestamp)}`
            : ult ? `offline · ${relTime(ult.timestamp)}` : escapeHtml(t.localizacao ?? 'sem dados');
          return `
            <button class="picker-item ${isActive ? 'active' : ''}" data-id="${t.id}">
              <div class="picker-icon">${icone}</div>
              <div class="picker-info">
                <div class="picker-name">${escapeHtml(t.nome)}</div>
                <div class="picker-sub">${sub}</div>
              </div>
              <div class="picker-state" style="background:${info.color}" title="${info.label}"></div>
            </button>
          `;
        })
        .join('')}
    </div>
  `;

  el.querySelector<HTMLButtonElement>('[data-all]')?.addEventListener('click', () => p.onShowAll());
  el.querySelectorAll<HTMLButtonElement>('[data-id]').forEach((btn) => {
    btn.addEventListener('click', () => {
      const id = btn.dataset.id;
      if (id) p.onSelect(id);
    });
  });

  // Clicar no fundo da sidebar (fora dos botões) abre/fecha
  el.addEventListener('click', (e) => {
    const target = e.target as Element;
    if (!target.closest('[data-id]') && !target.closest('[data-all]')) {
      toggleCollapsed(el);
    }
  });
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}
