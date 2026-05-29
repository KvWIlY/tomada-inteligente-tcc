// =============================================
// Toast (notificacao temporaria no canto inferior)
// =============================================

let timer: number | null = null;

export function renderToast(el: HTMLElement, msg: string | null): void {
  if (!msg) {
    el.classList.remove('show');
    el.innerHTML = '';
    return;
  }
  el.innerHTML = `<div class="toast-msg">${escapeHtml(msg)}</div>`;
  el.classList.add('show');
  if (timer !== null) clearTimeout(timer);
  timer = window.setTimeout(() => {
    el.classList.remove('show');
  }, 2800);
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}
