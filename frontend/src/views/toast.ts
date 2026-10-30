// =============================================
// Aviso efemero (`toaster` do Basecoat) + dialogo modal (`<dialog>`)
// =============================================

import { icon } from '../icons';
import { escapeHtml } from './components';

export interface ToastData {
  msg: string;
  kind: 'ok' | 'alert';
}

/** O `#toaster` do Basecoat ganha um metodo `toast()` ao ser inicializado. */
type Toaster = HTMLElement & { toast?: (config: Record<string, unknown>) => void };

let ultimoToast: ToastData | null = null;

/**
 * Empurra o aviso para o `#toaster`.
 *
 * O sumico sozinho (FR-005e item 7) passou a ser do Basecoat - 3 s para aviso
 * comum, 5 s para erro, com o relogio pausado enquanto o ponteiro esta em cima.
 * Por isso `main.ts` nao tem mais `setTimeout` proprio: o estado guarda o ultimo
 * aviso e este comparador de identidade evita reempurrar o mesmo objeto a cada
 * re-render (o polling redesenha a tela a cada 15 s).
 *
 * `title` e `description` entram como HTML cru no componente - escapar aqui nao
 * e zelo, e a unica barreira: o texto inclui nome de tomada digitado pelo dono.
 */
export function renderToast(el: HTMLElement, toast: ToastData | null): void {
  if (toast === ultimoToast) return;
  ultimoToast = toast;
  if (!toast) return;
  (el as Toaster).toast?.({
    category: toast.kind === 'alert' ? 'error' : 'success',
    title: escapeHtml(toast.msg),
  });
}

export interface ModalData {
  title: string;
  body: string;
  confirmLabel: string;
  danger: boolean;
  onConfirm: () => void;
  onCancel: () => void;
  /** Se presente, substitui o corpo de texto (o chamador escapa o que insere). */
  bodyHtml?: string;
  /** Classes extras no cartao do dialogo (variantes de largura, por exemplo). */
  className?: string;
  /** Botão à esquerda no rodapé (ex.: "Copiar"). Substitui o "Cancelar". */
  secondaryLabel?: string;
  onSecondary?: () => void;
  /** Ações de botões DENTRO do bodyHtml: qualquer `[data-action]` clicado chama
   *  `onAction(action, value)` com os atributos `data-action`/`data-value`. */
  onAction?: (action: string, value: string) => void;
}

/**
 * Dialogo modal sobre o `<dialog>` nativo, com o vestuario `alert-dialog` do
 * Basecoat. Trocar a `div` com veu proprio pelo elemento nativo entrega de
 * graca o que era imitado a mao: camada de topo (sem disputa de `z-index`),
 * `::backdrop`, foco preso dentro do dialogo e Esc.
 */
let ultimoModal: ModalData | null = null;

export function renderModal(el: HTMLElement, modal: ModalData | null): void {
  // Sem esta guarda o dialogo seria reconstruido a cada tick do polling (15 s),
  // apagando o que o dono ja tivesse digitado no campo de renomear.
  if (modal === ultimoModal) return;
  ultimoModal = modal;

  if (!modal) {
    el.querySelector('dialog')?.close();
    el.innerHTML = '';
    return;
  }

  const glyph = modal.danger ? icon('warn', 22) : icon('check', 22);
  const corpo = modal.bodyHtml ?? `<p data-description>${escapeHtml(modal.body)}</p>`;
  const temSecundario = Boolean(modal.secondaryLabel);
  const esquerda = temSecundario
    ? `<button type="button" class="btn" data-variant="outline" data-role="secondary">${escapeHtml(modal.secondaryLabel!)}</button>`
    : `<button type="button" class="btn" data-variant="outline" data-role="cancel">Cancelar</button>`;

  el.innerHTML = `
    <dialog class="alert-dialog" aria-labelledby="modal-titulo">
      <article class="${escapeHtml(modal.className ?? '')}">
        <header>
          <figure class="grid size-11 place-items-center rounded-xl ${modal.danger ? 'bg-red-bg text-red-ink' : 'bg-green-bg text-green-ink'}">${glyph}</figure>
          <h2 id="modal-titulo">${escapeHtml(modal.title)}</h2>
        </header>
        <section class="px-6 text-sm text-ink-soft">${corpo}</section>
        <footer class="${temSecundario ? '!justify-between' : ''}">
          ${esquerda}
          <button type="button" class="btn" ${modal.danger ? 'data-variant="destructive"' : ''} data-role="confirm">${escapeHtml(modal.confirmLabel)}</button>
        </footer>
      </article>
    </dialog>`;

  const dlg = el.querySelector('dialog')!;
  dlg.showModal();

  dlg.querySelector('[data-role="confirm"]')?.addEventListener('click', () => modal.onConfirm());
  dlg.querySelector('[data-role="cancel"]')?.addEventListener('click', () => modal.onCancel());
  dlg.querySelector('[data-role="secondary"]')?.addEventListener('click', () => modal.onSecondary?.());
  dlg.querySelectorAll<HTMLElement>('[data-action]').forEach((btn) => {
    btn.addEventListener('click', () =>
      modal.onAction?.(btn.dataset.action ?? '', btn.dataset.value ?? ''),
    );
  });
  // Esc e clique fora fecham pelo mesmo caminho do "Cancelar".
  dlg.addEventListener('cancel', (e) => {
    e.preventDefault();
    modal.onCancel();
  });
  dlg.addEventListener('click', (e) => {
    if (e.target === dlg) modal.onCancel();
  });

  // Dialogo com campo (renomear): foca, seleciona o texto e deixa o Enter
  // confirmar - o `<dialog>` nativo nao submete nada sozinho.
  const campo = dlg.querySelector<HTMLInputElement>('input[data-autofocus]');
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
