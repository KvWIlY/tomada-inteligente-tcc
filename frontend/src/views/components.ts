// =============================================
// Pecas reutilizaveis (string builders) — Basecoat + tokens do projeto
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

/** Icone do aparelho a partir do campo `icone` da tomada. */
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
  return `<span class="badge badge--${estado} gap-1.5">${stateDot(estado)}${STATE_LABEL[estado]}</span>`;
}

export function critBadge(): string {
  return `<span class="badge badge--crit gap-1">${icon('warn', 12, 2.2)}Crítico</span>`;
}

/**
 * Switch do Basecoat: `<input type=checkbox role=switch class=input>`.
 *
 * Ate a 007 isto era um `<button aria-pressed>` proprio, porque a versao
 * anterior da biblioteca nao trazia switch. A 1.0.2 traz, e o controle nativo
 * ganha de graca o que o botao imitava a mao: foco, teclado e estado de
 * formulario.
 */
export function toggle(on: boolean, attrs = '', label = 'Ligar ou desligar'): string {
  return `<input type="checkbox" role="switch" class="input" ${on ? 'checked' : ''} ${attrs} aria-label="${escapeHtml(label)}" />`;
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

/** Fundo/tinta do icone do evento. Ambos os pares tem variante `-ink` para
 *  texto: o glifo aqui e grafico sobre fundo tenue, e a `-ink` passa nos dois. */
function feedClass(sev: Evento['severidade']): string {
  if (sev === 'warn') return 'bg-amber-bg text-amber-ink';
  if (sev === 'low') return 'bg-red-bg text-red-ink';
  return 'bg-green-bg text-green-ink';
}

/**
 * Feed "Atividade do Otimizador".
 *
 * GATE DO PRINCIPIO VII: este e o unico lugar onde o dono ve POR QUE o solver
 * decidiu o que decidiu. O campo `detalhes` carrega essa justificativa e nao
 * pode sumir na migracao — nem quando vem vazio, caso em que a linha
 * simplesmente nao e emitida (antes saia uma `<div>` vazia ocupando altura).
 */
export function eventsFeed(eventos: Evento[]): string {
  if (eventos.length === 0) {
    return `
      <div class="empty py-10">
        <header>
          <figure class="mb-4 grid size-14 place-items-center rounded-2xl bg-lav text-ink-faint">${icon('cpu', 24)}</figure>
          <h3 class="text-lg font-bold tracking-tight">Sem atividade do otimizador ainda</h3>
          <p class="mt-2 text-sm text-ink-soft">As decisões do MILP aparecem aqui assim que a primeira tomada
             reportar um ciclo — inclusive o motivo de cada chaveamento.</p>
        </header>
      </div>`;
  }
  return `<div class="item-group">${eventos
    .map(
      (e) => `
      <div class="item border-0 border-t border-line-soft px-0 py-3.5 first:border-t-0" data-size="sm">
        <figure class="size-8 rounded-[10px] ${feedClass(e.severidade)}">${feedIcon(e)}</figure>
        <section>
          <h4 class="text-sm font-medium text-ink">${escapeHtml(e.titulo)}</h4>
          ${e.detalhes ? `<p class="text-ink-soft">${escapeHtml(e.detalhes)}</p>` : ''}
        </section>
        <aside class="tnum shrink-0 text-xs whitespace-nowrap text-ink-faint">${relTime(e.timestamp)}</aside>
      </div>`,
    )
    .join('')}</div>`;
}
