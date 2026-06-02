// =============================================
// SVG icons (porte do set do design handoff)
// =============================================
//
// Em vanilla TS sem JSX, montamos o SVG como string e injetamos via innerHTML.
// `icon(name, size, sw)` devolve o <svg> completo; os paths abaixo usam o
// viewBox 0 0 24 24 padrao do handoff (traco, herda currentColor).

const PATHS: Record<string, string> = {
  bolt:  '<path d="M13 2 4 14h7l-1 8 9-12h-7l1-8Z"/>',
  grid:  '<rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/>',
  fridge:'<rect x="6" y="2.5" width="12" height="19" rx="2.5"/><path d="M6 10h12M9.5 6v1.5M9.5 13v2"/>',
  wifi:  '<path d="M5 12.5a10 10 0 0 1 14 0M8 16a5 5 0 0 1 8 0"/><circle cx="12" cy="19.5" r="0.8" fill="currentColor"/>',
  wind:  '<path d="M3 8h10a2.5 2.5 0 1 0-2.5-2.5M3 16h13a3 3 0 1 1-3 3M3 12h7"/>',
  tv:    '<rect x="3" y="5" width="18" height="12" rx="2"/><path d="M8 21h8M12 17v4"/>',
  monitor:'<rect x="3" y="4" width="18" height="13" rx="2"/><path d="M8 21h8M12 17v4"/>',
  microwave:'<rect x="2.5" y="5" width="19" height="14" rx="2"/><rect x="5.5" y="8" width="9" height="8" rx="1"/><path d="M18 9v6"/>',
  coffee:'<path d="M5 9h12v5a4 4 0 0 1-4 4H9a4 4 0 0 1-4-4V9ZM17 10h2a2.5 2.5 0 0 1 0 5h-2M8 5V3M12 5V3"/>',
  washer:'<rect x="5" y="3" width="14" height="18" rx="2.5"/><circle cx="12" cy="13" r="4"/><path d="M8 6.5h.01M11 6.5h.01"/>',
  plug:  '<path d="M9 2v6M15 2v6M6 8h12v3a6 6 0 0 1-12 0V8ZM12 17v5"/>',
  printer:'<path d="M6 9V2h12v7"/><path d="M6 18H4a2 2 0 0 1-2-2v-5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2h-2"/><rect x="6" y="14" width="12" height="8" rx="1"/>',
  charger:'<rect x="6" y="7" width="12" height="14" rx="2"/><path d="M9 4v3M15 4v3M10 13l2 2 3-4"/>',
  bed:   '<path d="M3 18v-6h18v6M3 12V7M21 12v6M3 12h18M6.5 12v-2.5a1.5 1.5 0 0 1 1.5-1.5h2"/>',
  logout:'<path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4M16 17l5-5-5-5M21 12H9"/>',
  gauge: '<path d="M12 14a2 2 0 1 0 0-4 2 2 0 0 0 0 4ZM13.4 10.6 17 7M3.5 18a9 9 0 1 1 17 0"/>',
  info:  '<circle cx="12" cy="12" r="9"/><path d="M12 11v5M12 7.5h.01"/>',
  plus:  '<path d="M12 5v14M5 12h14"/>',
  warn:  '<path d="M12 9v4M12 17h.01M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0Z"/>',
  check: '<path d="m20 6-11 11-5-5"/>',
  cpu:   '<rect x="6" y="6" width="12" height="12" rx="2"/><path d="M9 1v3M15 1v3M9 20v3M15 20v3M1 9h3M1 15h3M20 9h3M20 15h3"/>',
  ghost: '<path d="M5 21V9a7 7 0 0 1 14 0v12l-3-2-2 2-2-2-2 2-3-2Z"/><circle cx="9.5" cy="10" r="0.6" fill="currentColor"/><circle cx="14.5" cy="10" r="0.6" fill="currentColor"/>',
  swap:  '<path d="M7 4 3 8l4 4M3 8h13M17 20l4-4-4-4M21 16H8"/>',
  cog:   '<circle cx="12" cy="12" r="3.2"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-2.82 1.17V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 7 19.4l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.6 13.8H4.5a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 6.3 7l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 12 4.6V4.5a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 2.82 1.17l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 11h.1a2 2 0 1 1 0 4h-.1Z"/>',
  cloud: '<path d="M17.5 19a4.5 4.5 0 1 0 0-9h-1.8a7 7 0 1 0-13.4 4"/>',
  menu:  '<path d="M4 7h16M4 12h16M4 17h16"/>',
};

/** Devolve o <svg> completo de um icone do set. Fallback = plug. */
export function icon(name: string, size = 18, sw = 2): string {
  const paths = PATHS[name] ?? PATHS.plug;
  return `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="${sw}" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${paths}</svg>`;
}
