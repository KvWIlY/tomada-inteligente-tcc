// =============================================
// Sidebar (264 px) — componente `sidebar` do Basecoat
// =============================================
//
// O elemento `.sidebar` vive no `index.html`. Usamos o CSS do componente, mas
// NAO o JS dele — quem abre e fecha e o `main.ts` (ver "Barra lateral" la).
// Esta funcao so preenche o `<nav>`: nao mexe em `className` nem em
// `aria-hidden`.

import { icon } from '../icons';
import type { Tomada } from '../types';
import { applianceIcon, escapeHtml } from './components';

export interface SidebarProps {
  tomadas: Tomada[];
  selectedId: string | null; // null = visão geral
  total: number;             // consumo total agora (W)
  userEmail: string | null;  // dono logado
  onSelect: (id: string | null) => void; // null = overview
  onInfo: (msg: string) => void;
  onRename?: (id: string) => void; // lapis discreto no item (renomeia a tomada)
  onLogout?: () => void;
  onAddDevice?: () => void;  // gera claim-code
}

export function renderSidebar(el: HTMLElement, p: SidebarProps): void {
  const navItems = p.tomadas
    .map((t) => {
      const active = t.id === p.selectedId;
      const w = t.estado === 'OFF' ? '—' : `${Math.round(t.p_medida_atual)} W`;
      return `
        <li class="group">
          <button type="button" data-id="${t.id}" ${active ? 'data-active="true"' : ''}>
            ${applianceIcon(t)}<span>${escapeHtml(t.nome)}</span>
            <small class="tnum ms-auto pe-6 text-xs font-bold text-ink-faint group-focus-within:invisible group-hover:invisible">${w}</small>
          </button>
          <button type="button" data-edit="${t.id}" title="Renomear tomada" aria-label="Renomear ${escapeHtml(t.nome)}"
            class="btn absolute end-1 top-1/2 size-7 -translate-y-1/2 rounded-md p-0 text-ink-faint opacity-0 hover:bg-green-bg hover:text-green focus-visible:opacity-100 group-hover:opacity-100"
            data-variant="ghost" data-size="icon-sm">${icon('edit', 16)}</button>
        </li>`;
    })
    .join('');

  el.innerHTML = `
    <nav aria-label="Navegação principal">
      <header class="border-b border-sidebar-border !p-5">
        <div class="flex items-center gap-3">
          <span class="grid size-10 shrink-0 place-items-center rounded-md bg-green text-primary-foreground">${icon('bolt', 20)}</span>
          <div class="min-w-0">
            <div class="truncate text-lg leading-none font-bold tracking-tight">SmartHome</div>
            <div class="mt-1 text-xs font-medium text-ink-faint">Gerenciador de Energia</div>
          </div>
        </div>
      </header>

      <section>
        <div role="group">
          <ul>
            <li>
              <button type="button" data-overview ${p.selectedId === null ? 'data-active="true"' : ''}>
                ${icon('grid')}<span>Visão Geral</span>
                <small class="tnum ms-auto text-xs font-bold text-ink-faint">${Math.round(p.total).toLocaleString('pt-BR')} W</small>
              </button>
            </li>
          </ul>
          <h3 class="mt-3">Tomadas · ${p.tomadas.length}</h3>
          <ul>${navItems}</ul>
        </div>
      </section>

      <footer class="border-t border-sidebar-border">
        <button type="button" class="btn w-full justify-start gap-2 text-green hover:bg-green-bg hover:text-green-deep" data-variant="ghost" data-add>
          ${icon('plus', 18)}Adicionar Novo Dispositivo
        </button>
        ${
          p.onLogout
            ? `<button type="button" class="btn w-full justify-start gap-2 text-ink-soft" data-variant="ghost" data-logout>
                 ${icon('logout')}<span class="min-w-0 truncate">${escapeHtml(p.userEmail ?? 'Sair')}</span>
               </button>`
            : ''
        }
      </footer>
    </nav>`;

  el.querySelector('[data-overview]')?.addEventListener('click', () => p.onSelect(null));
  el.querySelectorAll<HTMLButtonElement>('button[data-id]').forEach((btn) => {
    btn.addEventListener('click', () => {
      const id = btn.dataset.id;
      if (id) p.onSelect(id);
    });
  });
  // Lapis (aparece no hover/foco, sobre o W): renomeia sem selecionar a tomada.
  // E um `<button>` de verdade, e nao um `<span>` como ate a 007: so assim ele
  // e alcancavel por teclado (FR-015).
  el.querySelectorAll<HTMLButtonElement>('button[data-edit]').forEach((btn) => {
    btn.addEventListener('click', (e) => {
      e.stopPropagation();
      const id = btn.dataset.edit;
      if (id) p.onRename?.(id);
    });
  });
  el.querySelector('[data-add]')?.addEventListener('click', () => {
    if (p.onAddDevice) p.onAddDevice();
    else p.onInfo('Parear novo dispositivo — use o provisionamento do firmware.');
  });
  el.querySelector('[data-logout]')?.addEventListener('click', () => p.onLogout?.());
}
