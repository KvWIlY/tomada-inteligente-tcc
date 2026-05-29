// =============================================
// SVG icons como strings (zero deps)
// =============================================
//
// Por que strings? Em vanilla TS sem JSX, montar SVGs com createElementNS
// e MUITO verboso. Templates de string permitem injetar diretamente em
// `innerHTML` -- igual o React faz com dangerouslySetInnerHTML.

const svg = (
  width: number,
  height: number,
  paths: string,
  strokeWidth = 2,
): string =>
  `<svg width="${width}" height="${height}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="${strokeWidth}" stroke-linecap="round" stroke-linejoin="round">${paths}</svg>`;

export const ICONS: Record<string, string> = {
  // === Genericos (16px) ===
  power: svg(16, 16, '<path d="M12 2v10"/><path d="M18.4 6.6a9 9 0 1 1-12.77.04"/>'),
  bolt: svg(16, 16, '<polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/>'),
  leaf: svg(
    16, 16,
    '<path d="M11 20A7 7 0 0 1 9.8 6.1C15.5 5 17 4.48 19.2 2c1 1.5 2 4 2 6 0 5-3 11-12 12"/><path d="M2 21c0-3 1.85-5.36 5.08-6"/>',
  ),
  swap: svg(
    16, 16,
    '<polyline points="17 1 21 5 17 9"/><path d="M3 11V9a4 4 0 0 1 4-4h14"/><polyline points="7 23 3 19 7 15"/><path d="M21 13v2a4 4 0 0 1-4 4H3"/>',
  ),
  clock: svg(16, 16, '<circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/>'),
  warn: svg(
    16, 16,
    '<path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/>',
  ),
  cog: svg(
    16, 16,
    '<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>',
  ),
  cloud: svg(
    16, 16,
    '<path d="M17.5 19a4.5 4.5 0 1 0 0-9h-1.8a7 7 0 1 0-13.4 4"/><polyline points="16 16 12 12 8 16"/><line x1="12" y1="12" x2="12" y2="21"/>',
  ),
  arrowUp: svg(10, 10, '<polyline points="18 15 12 9 6 15"/>', 3),
  arrowDown: svg(10, 10, '<polyline points="6 9 12 15 18 9"/>', 3),
  chevronLeft: svg(16, 16, '<polyline points="15 18 9 12 15 6"/>', 2),
  chevronRight: svg(16, 16, '<polyline points="9 18 15 12 9 6"/>', 2),
  check: svg(14, 14, '<polyline points="20 6 9 17 4 12"/>', 3),

  // === Aparelhos (20px) ===
  plug: svg(
    20, 20,
    '<path d="M9 2v6"/><path d="M15 2v6"/><path d="M6 8h12v4a6 6 0 0 1-12 0z"/><path d="M12 18v4"/>',
  ),
  printer: svg(
    20, 20,
    '<polyline points="6 9 6 2 18 2 18 9"/><path d="M6 18H4a2 2 0 0 1-2-2v-5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2h-2"/><rect x="6" y="14" width="12" height="8"/>',
  ),
  tv: svg(
    20, 20,
    '<rect x="2" y="7" width="20" height="13" rx="2"/><polyline points="17 2 12 7 7 2"/>',
  ),
  fridge: svg(
    20, 20,
    '<rect x="5" y="2" width="14" height="20" rx="2"/><line x1="5" y1="10" x2="19" y2="10"/><line x1="9" y1="6" x2="9" y2="7"/><line x1="9" y1="14" x2="9" y2="17"/>',
  ),
  charger: svg(
    20, 20,
    '<rect x="6" y="7" width="12" height="14" rx="2"/><path d="M9 4v3"/><path d="M15 4v3"/><path d="M10 13l2 2 3-4"/>',
  ),
};
