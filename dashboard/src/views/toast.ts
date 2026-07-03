// =============================================
// Toast (canto inferior, efêmero) + Modal de confirmação
// =============================================

import { icon } from '../icons';
import { escapeHtml } from './components';

export interface ToastData {
  msg: string;
  kind: 'ok' | 'alert';
}

export function renderToast(el: HTMLElement, toast: ToastData | null): void {
  if (!toast) {
    el.innerHTML = '';
    return;
  }
  const ic = toast.kind === 'alert' ? icon('warn', 16, 2.2) : icon('check', 16, 2.6);
  el.innerHTML = `
    <div class="toast-wrap">
      <div class="toast toast--${toast.kind}">
        <span class="toast__ic">${ic}</span>
        <span>${escapeHtml(toast.msg)}</span>
      </div>
    </div>`;
}

export interface ModalData {
  title: string;
  body: string;
  confirmLabel: string;
  danger: boolean;
  onConfirm: () => void;
  onCancel: () => void;
  /** Se presente, substitui o corpo de texto (o chamador é responsável por escapar). */
  bodyHtml?: string;
  /** Classe extra no `.modal` (variantes visuais, ex.: `modal--device`). */
  className?: string;
  /** Botão à esquerda no rodapé (ex.: "Copiar"). Substitui o "Cancelar" e faz o
   *  rodapé virar `space-between` (secundário à esquerda, confirmar à direita). */
  secondaryLabel?: string;
  onSecondary?: () => void;
}

export function renderModal(el: HTMLElement, modal: ModalData | null): void {
  if (!modal) {
    el.innerHTML = '';
    return;
  }
  const iconKind = modal.danger ? 'danger' : 'ok';
  const glyph = modal.danger ? icon('warn', 24, 2.2) : icon('check', 24, 2.6);
  const corpo = modal.bodyHtml ?? `<p class="modal__body">${escapeHtml(modal.body)}</p>`;
  const temSecundario = Boolean(modal.secondaryLabel);
  const esquerda = temSecundario
    ? `<button class="btn btn--ghost" data-role="secondary">${escapeHtml(modal.secondaryLabel!)}</button>`
    : `<button class="btn btn--ghost" data-role="cancel">Cancelar</button>`;
  el.innerHTML = `
    <div class="modal-scrim" data-role="scrim">
      <div class="modal${modal.className ? ` ${modal.className}` : ''}" role="dialog" aria-modal="true">
        <div class="modal__head">
          <div class="modal__icon modal__icon--${iconKind}">${glyph}</div>
          <h3 class="modal__title">${escapeHtml(modal.title)}</h3>
        </div>
        ${corpo}
        <div class="modal__actions${temSecundario ? ' modal__actions--split' : ''}">
          ${esquerda}
          <button class="btn btn--${modal.danger ? 'danger' : 'green'}" data-role="confirm">${escapeHtml(modal.confirmLabel)}</button>
        </div>
      </div>
    </div>`;

  el.querySelector('[data-role="confirm"]')?.addEventListener('click', () => modal.onConfirm());
  el.querySelector('[data-role="cancel"]')?.addEventListener('click', () => modal.onCancel());
  el.querySelector('[data-role="secondary"]')?.addEventListener('click', () => modal.onSecondary?.());
  el.querySelector('[data-role="scrim"]')?.addEventListener('click', (e) => {
    if (e.target === e.currentTarget) modal.onCancel();
  });

  // Modal com campo (ex.: renomear): foca + seleciona o texto e deixa o Enter confirmar.
  const campo = el.querySelector<HTMLInputElement>('input[autofocus]');
  if (campo) {
    campo.focus();
    campo.select();
    campo.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') {
        e.preventDefault();
        modal.onConfirm();
      }
    });
  }
}
