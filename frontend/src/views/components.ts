// =============================================
// Peças reutilizáveis (string builders) · idioma verde/branco
// =============================================

import { icon } from '../icons';
import type { Estado, Evento, Tomada } from '../types';
import { relTime } from '../utils';

const STATE_LABEL: Record<Estado, string> = { ON: 'Ligada', STD: 'Standby', OFF: 'Desligada' };

export function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

/** Ícone do aparelho a partir do campo `icone` da tomada. */
export function applianceIcon(t: Pick<Tomada, 'icone'>, size = 18): string {
  return icon(t.icone, size);
}

export function stateDot(estado: Estado, live = false): string {
  return `<span class="dot dot--${estado}${live && estado === 'ON' ? ' dot--live' : ''}"></span>`;
}

/**
 * Badge de estado. A FORMA vem do `.badge` do Basecoat (altura, raio,
 * tipografia, transicao); a COR vem de `.badge--{estado}`, que consome nossos
 * tokens semanticos. Basecoat aplica `bg-primary` por padrao no `.badge` sem
 * `data-variant`; nossas regras sao sem camada e por isso vencem.
 *
 * O rotulo textual ao lado do ponto colorido nao e enfeite: FR-002d proibe
 * transmitir informacao so por cor.
 */
export function badge(estado: Estado): string {
  return `<span class="badge badge--${estado}">${stateDot(estado)}${STATE_LABEL[estado]}</span>`;
}

export function critBadge(): string {
  return `<span class="badge badge--crit">${icon('warn', 12, 2.2)}Crítico</span>`;
}

/**
 * Toggle pilula. O Basecoat **nao tem** componente de switch — a lista de
 * componentes dele vai de `.accordion` a `.textarea` sem passar por switch —
 * entao este fica proprio, com os mesmos tokens (FR-003a).
 */
export function toggle(on: boolean, attrs = ''): string {
  return `<button class="toggle${on ? ' toggle--on' : ''}" ${attrs} aria-pressed="${on}" aria-label="Ligar ou desligar"><span class="toggle__knob"></span></button>`;
}

function feedIcon(e: Evento): string {
  switch (e.tipo) {
    case 'chaveamento': return icon('swap', 16);
    case 'config':      return icon('cpu', 16);
    case 'inviabilidade': return icon('warn', 16, 2.2);
    case 'sync_error':  return icon('cloud', 16);
    case 'cold_start':  return icon('check', 16, 2.6);
    default:            return icon('bolt', 16);
  }
}

function feedClass(sev: Evento['severidade']): string {
  if (sev === 'warn') return 'fic--warn';
  if (sev === 'low') return 'fic--err';
  return 'fic--info';
}

/** Feed "Atividade do Otimizador". */
export function eventsFeed(eventos: Evento[]): string {
  if (eventos.length === 0) {
    return `<div class="empty__sub" style="text-align:center;padding:22px 0">Sem atividade do otimizador ainda.</div>`;
  }
  return `<div class="feed">${eventos
    .map(
      (e) => `
      <div class="fitem">
        <span class="fitem__icon ${feedClass(e.severidade)}">${feedIcon(e)}</span>
        <div class="fitem__body">
          <div class="fitem__title">${escapeHtml(e.titulo)}</div>
          <div class="fitem__detail">${escapeHtml(e.detalhes ?? '')}</div>
        </div>
        <span class="fitem__time tnum">${relTime(e.timestamp)}</span>
      </div>`,
    )
    .join('')}</div>`;
}
