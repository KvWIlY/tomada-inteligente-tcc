// =============================================
// Ícones — Lucide (ISC), inlinados
// =============================================
//
// Set: Lucide (https://lucide.dev), o mesmo que o shadcn/ui usa. Os arquivos de
// origem e o aviso de copyright ISC vivem em `public/icons/` — ver
// `public/icons/ORIGEM.md` para o mapa nome-no-codigo -> nome-no-lucide.
//
// Por que INLINE e nao <img src="/icons/x.svg">: as views montam marcacao como
// string, de forma sincrona, e nao podem esperar uma requisicao. Inlinar tambem
// deixa o icone herdar `currentColor`, que e o que permite um mesmo arquivo
// servir os dois temas (FR-002a) e os estados de badge.
//
// Trocamos o Flaticon por este set em 2026-08-24: o Flaticon exige conta,
// devolve 403 a acesso automatizado, e seus termos nao puderam ser lidos para
// verificar atribuicao e direito de redistribuicao. A ISC do Lucide e legivel,
// permite redistribuir, e a unica obrigacao — preservar o aviso de copyright —
// esta cumprida em `public/icons/LICENSE`.
//
// Apenas os 13 icones efetivamente usados (FR-004d).

const PATHS: Record<string, string> = {
  bolt: '<path d="M15.914 4a1.5 1.5 0 00-2.474-1.561l-9 9A1.5 1.5 0 005.5 14h4.002a.5.5 0 01.471.666L8.086 20a1.5 1.5 0 002.475 1.56l9-9A1.5 1.5 0 0018.5 10h-3.997a.5.5 0 01-.472-.667z" />',
  check: '<path d="M20 6 9 17l-5-5" />',
  cloud: '<path d="M17.5 19H9a7 7 0 1 1 6.71-9h1.79a4.5 4.5 0 1 1 0 9Z" />',
  cpu: '<path d="M12 20v2" /> <path d="M12 2v2" /> <path d="M17 20v2" /> <path d="M17 2v2" /> <path d="M2 12h2" /> <path d="M2 17h2" /> <path d="M2 7h2" /> <path d="M20 12h2" /> <path d="M20 17h2" /> <path d="M20 7h2" /> <path d="M7 20v2" /> <path d="M7 2v2" /> <rect x="4" y="4" width="16" height="16" rx="2" /> <rect x="8" y="8" width="8" height="8" rx="1" />',
  edit: '<path d="M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z" /> <path d="m15 5 4 4" />',
  gauge: '<path d="m12 14 4-4" /> <path d="M3.34 19a10 10 0 1 1 17.32 0" />',
  grid: '<rect width="7" height="7" x="3" y="3" rx="1" /> <rect width="7" height="7" x="14" y="3" rx="1" /> <rect width="7" height="7" x="14" y="14" rx="1" /> <rect width="7" height="7" x="3" y="14" rx="1" />',
  info: '<circle cx="12" cy="12" r="10" /> <path d="M12 16v-4" /> <path d="M12 8h.01" />',
  logout: '<path d="m16 17 5-5-5-5" /> <path d="M21 12H9" /> <path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4" />',
  plug: '<path d="M12 22v-5" /> <path d="M15 8V2" /> <path d="M17 8a1 1 0 0 1 1 1v4a4 4 0 0 1-4 4h-4a4 4 0 0 1-4-4V9a1 1 0 0 1 1-1z" /> <path d="M9 8V2" />',
  plus: '<path d="M5 12h14" /> <path d="M12 5v14" />',
  swap: '<path d="M8 3 4 7l4 4" /> <path d="M4 7h16" /> <path d="m16 21 4-4-4-4" /> <path d="M20 17H4" />',
  warn: '<path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3" /> <path d="M12 9v4" /> <path d="M12 17h.01" />',
};

/** Devolve o <svg> completo de um icone do set. Fallback = plug. */
export function icon(name: string, size = 18, sw = 2): string {
  const paths = PATHS[name] ?? PATHS.plug;
  return `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="${sw}" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${paths}</svg>`;
}
