// =============================================
// Topbar (header com nome + status)
// =============================================

import { ICONS } from '../icons';
import type { Medicao, Residencia, Tomada } from '../types';
import { relTime } from '../utils';

export interface TopbarProps {
  residencia: Residencia | null;
  tomadas: Tomada[];
  medicoesPorTomada: Record<string, Medicao[]>;
  online: boolean;
}

export function renderTopbar(el: HTMLElement, p: TopbarProps): void {
  // Encontra a medicao MAIS RECENTE entre todas as tomadas
  let ultimoTs = 0;
  for (const t of p.tomadas) {
    const meds = p.medicoesPorTomada[t.id];
    if (meds && meds.length > 0) {
      const ult = meds[meds.length - 1].timestamp;
      if (ult > ultimoTs) ultimoTs = ult;
    }
  }
  const statusText = ultimoTs > 0
    ? `Online · medicoes ${relTime(ultimoTs)}`
    : (p.online ? 'Online · sem medicoes ainda' : 'Offline');
  const statusClass = p.online ? 'status-on' : 'status-off';

  const subText = p.residencia
    ? `${escapeHtml(p.residencia.nome)} · ${p.tomadas.length} dispositivo${p.tomadas.length === 1 ? '' : 's'}`
    : 'Carregando...';

  el.innerHTML = `
    <div class="topbar-left">
      <div class="topbar-logo">${ICONS.plug}</div>
      <div>
        <div class="topbar-title">Tomada Inteligente · TCC</div>
        <div class="topbar-sub">${subText}</div>
      </div>
    </div>
    <div class="topbar-right">
      <div class="topbar-status ${statusClass}">
        <span class="status-dot"></span>
        ${statusText}
      </div>
    </div>
  `;
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}
