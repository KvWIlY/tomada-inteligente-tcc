// =============================================
// Helpers compartilhados pelas views
// =============================================

/** Tempo relativo: "agora", "5 min atrás", "2h atrás", "ontem". */
export function relTime(unixSec: number): string {
  const diffSec = Math.floor(Date.now() / 1000) - unixSec;
  if (diffSec < 30) return 'agora';
  if (diffSec < 60) return `${diffSec}s atrás`;
  const min = Math.floor(diffSec / 60);
  if (min < 60) return `${min} min atrás`;
  const h = Math.floor(min / 60);
  if (h < 24) return `${h}h atrás`;
  const d = Math.floor(h / 24);
  if (d === 1) return 'ontem';
  return `${d} dias atrás`;
}
