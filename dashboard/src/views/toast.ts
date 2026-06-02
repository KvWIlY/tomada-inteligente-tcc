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
}

export function renderModal(el: HTMLElement, modal: ModalData | null): void {
  if (!modal) {
    el.innerHTML = '';
    return;
  }
  const iconKind = modal.danger ? 'danger' : 'ok';
  const glyph = modal.danger ? icon('warn', 24, 2.2) : icon('check', 24, 2.6);
  el.innerHTML = `
    <div class="modal-scrim" data-role="scrim">
      <div class="modal" role="dialog" aria-modal="true">
        <div class="modal__icon modal__icon--${iconKind}">${glyph}</div>
        <h3 class="modal__title">${escapeHtml(modal.title)}</h3>
        <p class="modal__body">${escapeHtml(modal.body)}</p>
        <div class="modal__actions">
          <button class="btn btn--ghost" data-role="cancel">Cancelar</button>
          <button class="btn btn--${modal.danger ? 'danger' : 'green'}" data-role="confirm">${escapeHtml(modal.confirmLabel)}</button>
        </div>
      </div>
    </div>`;

  el.querySelector('[data-role="confirm"]')?.addEventListener('click', () => modal.onConfirm());
  el.querySelector('[data-role="cancel"]')?.addEventListener('click', () => modal.onCancel());
  el.querySelector('[data-role="scrim"]')?.addEventListener('click', (e) => {
    if (e.target === e.currentTarget) modal.onCancel();
  });
}
