// =============================================
// Sidebar (264px): marca + navegação por tomada + rodapé
// =============================================

import { icon } from '../icons';
import type { Tomada } from '../types';
import { applianceIcon, escapeHtml } from './components';

export interface SidebarProps {
  tomadas: Tomada[];
  selectedId: string | null; // null = visão geral
  total: number;             // consumo total agora (W)
  navOpen: boolean;
  userEmail: string | null;  // dono logado
  onSelect: (id: string | null) => void; // null = overview
  onCloseNav: () => void;
  onInfo: (msg: string) => void;
  onRename?: (id: string) => void; // lapis discreto no item (renomeia a tomada)
  onLogout?: () => void;
  onAddDevice?: () => void;  // gera claim-code
}

export function renderSidebar(el: HTMLElement, p: SidebarProps): void {
  el.className = `l-sidebar${p.navOpen ? ' l-sidebar--open' : ''}`;

  const navItems = p.tomadas
    .map((t) => {
      const active = t.id === p.selectedId;
      const w = t.estado === 'OFF' ? '—' : `${Math.round(t.p_medida_atual)} W`;
      return `
        <button class="nav__item${active ? ' nav__item--active' : ''}" data-id="${t.id}">
          ${applianceIcon(t)}<span class="nav__name">${escapeHtml(t.nome)}</span>
          <span class="nav__edit" data-edit title="Renomear tomada" aria-label="Renomear tomada">${icon('edit', 16)}</span>
          <span class="nav__w tnum">${w}</span>
        </button>`;
    })
    .join('');

  el.innerHTML = `
    <div class="brand">
      <span class="brand__mark">${icon('bolt', 20)}</span>
      <div>
        <div class="brand__name">SmartHome</div>
        <div class="brand__sub">Gerenciador de Energia</div>
      </div>
    </div>

    <nav class="nav">
      <button class="nav__item${p.selectedId === null ? ' nav__item--active' : ''}" data-overview>
        ${icon('grid')}<span class="nav__name">Visão Geral</span>
        <span class="nav__w tnum">${p.total.toLocaleString('pt-BR')} W</span>
      </button>

      <div class="nav__label">Tomadas · ${p.tomadas.length}</div>
      ${navItems}
    </nav>

    <div class="side-foot">
      <button class="side-action side-action--primary" data-add>${icon('plus', 18, 2.2)}Adicionar Novo Dispositivo</button>
      ${p.onLogout ? `<button class="side-action side-action--muted" data-logout>${icon('logout')}<span class="nav__name">${escapeHtml(p.userEmail ?? 'Sair')}</span></button>` : ''}
    </div>`;

  el.querySelector('[data-overview]')?.addEventListener('click', () => {
    p.onSelect(null);
    p.onCloseNav();
  });
  el.querySelectorAll<HTMLButtonElement>('.nav__item[data-id]').forEach((btn) => {
    btn.addEventListener('click', () => {
      const id = btn.dataset.id;
      if (id) {
        p.onSelect(id);
        p.onCloseNav();
      }
    });
  });
  // Lapis (sobre o W, aparece no hover): renomeia sem disparar o select do item.
  el.querySelectorAll<HTMLElement>('.nav__edit[data-edit]').forEach((sp) => {
    sp.addEventListener('click', (e) => {
      e.stopPropagation();
      const id = sp.closest<HTMLElement>('.nav__item')?.dataset.id;
      if (id) p.onRename?.(id);
    });
  });
  el.querySelector('[data-add]')?.addEventListener('click', () => {
    if (p.onAddDevice) p.onAddDevice();
    else p.onInfo('Parear novo dispositivo — use o provisionamento do firmware.');
  });
  el.querySelector('[data-logout]')?.addEventListener('click', () => p.onLogout?.());
}
