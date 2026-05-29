// =============================================
// Painel de configuracao da tomada (P_limiar, T_limite, is_critical, schedule)
// =============================================

import { ICONS } from '../icons';
import type { Config, Residencia, ScheduleStyle, Tomada } from '../types';
import { relTime } from '../utils';

export interface ConfigProps {
  tomada: Tomada;
  residencia: Residencia | null;
  scheduleStyle: ScheduleStyle;
  onChangeScheduleStyle: (s: ScheduleStyle) => void;
  onSave: (c: Config, pMax?: number) => void;
  saving: boolean;
}

// Estado local do form (rascunho) -- mantido por um Map por id de tomada
const drafts = new Map<string, Config>();

export function renderConfig(el: HTMLElement, p: ConfigProps): void {
  // Inicializa rascunho se nao existir ou se a config_version mudou (recebeu update do banco)
  const cacheKey = `${p.tomada.id}@${p.tomada.config_version}`;
  if (!drafts.has(cacheKey)) {
    // Limpa rascunhos antigos da mesma tomada
    for (const k of drafts.keys()) {
      if (k.startsWith(`${p.tomada.id}@`)) drafts.delete(k);
    }
    drafts.set(cacheKey, structuredClone(p.tomada.config));
  }
  const draft = drafts.get(cacheKey)!;
  const dirty = JSON.stringify(draft) !== JSON.stringify(p.tomada.config);
  const currentHour = new Date().getHours();
  const tLimiteMin = Math.round(draft.t_limite / 60);

  el.innerHTML = `
    <section class="section">
      <div class="section-head">
        <div>
          <h2 class="section-title">${ICONS.cog} Configuracao da tomada</h2>
          <p class="section-sub">Editado aqui · sincronizado via Supabase no proximo polling do ESP32</p>
        </div>
      </div>

      <div class="form">
        <!-- P_limiar auto-detectado (somente leitura) -->
        <div class="field">
          <div class="field-row">
            <div>
              <div class="field-label">P_limiar</div>
              <div class="field-help">
                ${p.tomada.config.p_limiar_detectado_auto
                  ? `Detectado automaticamente: <strong>${p.tomada.config.p_limiar_detectado_auto.toFixed(1)} W</strong>`
                  : 'Aguardando histotico suficiente para detectar automaticamente (100+ medicoes)'}
              </div>
            </div>
          </div>
        </div>

        <!-- T_limite -->
        <div class="field">
          <div class="field-row">
            <div>
              <div class="field-label">T_limite</div>
              <div class="field-help">Tempo em standby ate classificar como consumo fantasma</div>
            </div>
            <div class="input-row">
              <input type="number" class="num-input" id="cfg-tlimite"
                     min="1" max="240" step="1" value="${tLimiteMin}" />
              <span class="unit-tag">min</span>
            </div>
          </div>
        </div>

        <!-- Critico -->
        <div class="field">
          <div class="field-row">
            <div>
              <div class="field-label">Aparelho critico</div>
              <div class="field-help">${
                draft.is_critical
                  ? 'Tomada nunca desliga · ignora schedule e P_limiar'
                  : 'Solver decide normalmente conforme historico e schedule'
              }</div>
            </div>
            <label class="switch">
              <input type="checkbox" id="cfg-critical" ${draft.is_critical ? 'checked' : ''} />
              <span class="switch-track"></span>
            </label>
          </div>
        </div>

        <!-- W_i (peso de criticidade ILP) -->
        <div class="field">
          <div class="field-row">
            <div>
              <div class="field-label">Peso de criticidade (W<sub>i</sub>)</div>
              <div class="field-help">Quanto maior, mais o solver evita desligar este aparelho</div>
            </div>
            <div class="input-row">
              <input type="range" class="range-input" id="cfg-wi"
                     min="1" max="1000" step="1" value="${draft.w_i ?? 10}" />
              <span class="unit-tag" id="cfg-wi-val">${draft.w_i ?? 10}</span>
            </div>
          </div>
          <div class="schedule-shortcuts">
            <button class="chip" data-wi="1">Lampada (1)</button>
            <button class="chip" data-wi="10">TV (10)</button>
            <button class="chip" data-wi="100">Roteador (100)</button>
            <button class="chip" data-wi="1000">Geladeira (1000)</button>
          </div>
        </div>

        <!-- P_max (capacidade do circuito) -->
        <div class="field">
          <div class="field-row">
            <div>
              <div class="field-label">Capacidade do circuito (P<sub>max</sub>)</div>
              <div class="field-help">Limite global de potencia para todas as tomadas da residencia</div>
            </div>
            <div class="input-row">
              <input type="number" class="num-input" id="cfg-pmax"
                     min="100" max="20000" step="10" value="${draft.p_max ?? 2540}" />
              <span class="unit-tag">W</span>
            </div>
          </div>
          <div class="schedule-shortcuts">
            <button class="chip" data-pmax="1270">10A/127V</button>
            <button class="chip" data-pmax="2540">20A/127V</button>
            <button class="chip" data-pmax="4400">20A/220V</button>
          </div>
        </div>

        <!-- Pi historico (somente leitura) -->
        <div class="field">
          <div class="field-row">
            <div>
              <div class="field-label">Probabilidade historica de uso (&Pi;)</div>
              <div class="field-help">Calculada pelo firmware via EMA · atualizada a cada ciclo</div>
            </div>
          </div>
          <div class="pi-chart">
            ${(draft.pi_historico ?? Array(24).fill(0.5)).map((v: number, h: number) => {
              const pct = Math.round(v * 100);
              const height = Math.max(2, pct);
              return `<div class="pi-bar" title="${h.toString().padStart(2, '0')}h: ${pct}%">
                <div class="pi-bar-fill" style="height:${height}%"></div>
                <div class="pi-bar-label">${h % 6 === 0 ? h.toString().padStart(2, '0') : ''}</div>
              </div>`;
            }).join('')}
          </div>
        </div>

        <!-- Schedule -->
        <div class="field" style="opacity:${draft.is_critical ? '0.4' : '1'};pointer-events:${draft.is_critical ? 'none' : 'auto'}">
          <div class="field-row">
            <div>
              <div class="field-label">Schedule de 24h</div>
              <div class="field-help">Horas em que o aparelho pode ficar ligado</div>
            </div>
            <div class="schedule-styles">
              ${(['grid', 'donut', 'bar'] as const)
                .map((k) => `<button class="${p.scheduleStyle === k ? 'active' : ''}" data-style="${k}">${k === 'grid' ? 'Grid' : k === 'donut' ? 'Donut' : 'Barra'}</button>`)
                .join('')}
            </div>
          </div>
          <div id="schedule-editor"></div>
          <div class="schedule-shortcuts">
            <button class="chip" data-shortcut="all">Sempre permitido</button>
            <button class="chip" data-shortcut="comercial">Comercial 8h-18h</button>
            <button class="chip" data-shortcut="diurno">Diurno 6h-22h</button>
            <button class="chip" data-shortcut="none">Limpar</button>
          </div>
        </div>

        <!-- Acoes -->
        <div class="form-actions">
          <div class="version-tag">
            <span>Versao atual</span>
            <span class="version-num">v${p.tomada.config_version}</span>
            <span>· atualizada ${relTime(Math.floor(new Date(p.tomada.updated_at).getTime() / 1000))}</span>
          </div>
          <div style="display:flex;gap:8px">
            <button class="btn" id="cfg-discard" ${dirty ? '' : 'disabled'}>Descartar</button>
            <button class="btn btn-primary" id="cfg-save" ${dirty && !p.saving ? '' : 'disabled'}>
              ${p.saving ? 'Salvando...' : `${ICONS.check} Salvar configuracao`}
            </button>
          </div>
        </div>
      </div>
    </section>
  `;

  // Renderiza o schedule editor selecionado
  const scheduleEl = el.querySelector<HTMLElement>('#schedule-editor');
  if (scheduleEl) {
    renderScheduleEditor(scheduleEl, p.scheduleStyle, draft.schedule, currentHour, (newSchedule) => {
      draft.schedule = newSchedule;
      renderConfig(el, p); // re-render
    });
  }

  // === Listeners ===

  // T_limite (em minutos -> guarda em segundos)
  el.querySelector<HTMLInputElement>('#cfg-tlimite')?.addEventListener('change', (e) => {
    const min = Number((e.target as HTMLInputElement).value);
    draft.t_limite = min * 60;
    renderConfig(el, p);
  });

  // is_critical
  el.querySelector<HTMLInputElement>('#cfg-critical')?.addEventListener('change', (e) => {
    draft.is_critical = (e.target as HTMLInputElement).checked;
    renderConfig(el, p);
  });

  // W_i slider
  el.querySelector<HTMLInputElement>('#cfg-wi')?.addEventListener('input', (e) => {
    const val = Number((e.target as HTMLInputElement).value);
    draft.w_i = val;
    const label = el.querySelector('#cfg-wi-val');
    if (label) label.textContent = String(val);
  });
  el.querySelector<HTMLInputElement>('#cfg-wi')?.addEventListener('change', () => {
    renderConfig(el, p);
  });
  el.querySelectorAll<HTMLButtonElement>('[data-wi]').forEach((btn) => {
    btn.addEventListener('click', () => {
      draft.w_i = Number(btn.dataset.wi);
      renderConfig(el, p);
    });
  });

  // P_max
  el.querySelector<HTMLInputElement>('#cfg-pmax')?.addEventListener('change', (e) => {
    draft.p_max = Number((e.target as HTMLInputElement).value);
    renderConfig(el, p);
  });
  el.querySelectorAll<HTMLButtonElement>('[data-pmax]').forEach((btn) => {
    btn.addEventListener('click', () => {
      draft.p_max = Number(btn.dataset.pmax);
      renderConfig(el, p);
    });
  });

  // Style buttons
  el.querySelectorAll<HTMLButtonElement>('[data-style]').forEach((btn) => {
    btn.addEventListener('click', () => p.onChangeScheduleStyle(btn.dataset.style as ScheduleStyle));
  });

  // Shortcuts
  el.querySelectorAll<HTMLButtonElement>('[data-shortcut]').forEach((btn) => {
    btn.addEventListener('click', () => {
      const k = btn.dataset.shortcut;
      if (k === 'all') draft.schedule = Array(24).fill(true);
      else if (k === 'comercial') draft.schedule = Array.from({ length: 24 }, (_, h) => h >= 8 && h < 18);
      else if (k === 'diurno') draft.schedule = Array.from({ length: 24 }, (_, h) => h >= 6 && h < 22);
      else if (k === 'none') draft.schedule = Array(24).fill(false);
      renderConfig(el, p);
    });
  });

  // Discard / Save
  el.querySelector<HTMLButtonElement>('#cfg-discard')?.addEventListener('click', () => {
    drafts.delete(cacheKey);
    renderConfig(el, p);
  });
  el.querySelector<HTMLButtonElement>('#cfg-save')?.addEventListener('click', () => {
    p.onSave(structuredClone(draft));
  });
}

// =============================================
// Schedule editors (3 estilos)
// =============================================

function renderScheduleEditor(
  el: HTMLElement,
  style: ScheduleStyle,
  schedule: boolean[],
  currentHour: number,
  onChange: (s: boolean[]) => void,
): void {
  const toggle = (h: number) => {
    const next = [...schedule];
    next[h] = !next[h];
    onChange(next);
  };

  if (style === 'grid') {
    el.innerHTML = `
      <div class="schedule">
        <div class="schedule-grid">
          ${schedule.map((on, h) =>
            `<div class="hr-cell ${on ? 'on' : ''} ${h === currentHour ? 'now' : ''}" data-h="${h}">
              ${h.toString().padStart(2, '0')}
            </div>`
          ).join('')}
        </div>
      </div>
    `;
    el.querySelectorAll<HTMLDivElement>('[data-h]').forEach((cell) => {
      cell.addEventListener('click', () => toggle(Number(cell.dataset.h)));
    });
    return;
  }

  if (style === 'bar') {
    el.innerHTML = `
      <div class="schedule">
        <div class="schedule-bar">
          <div class="bar-track">
            ${schedule.map((on, h) =>
              `<div class="bar-cell ${on ? 'on' : ''} ${h === currentHour ? 'now' : ''}"
                    data-h="${h}" title="${h.toString().padStart(2, '0')}:00 — ${on ? 'permitido' : 'bloqueado'}"></div>`
            ).join('')}
          </div>
          <div class="bar-axis">
            ${Array.from({ length: 24 }, (_, h) => `<span>${h % 6 === 0 ? h.toString().padStart(2, '0') : ''}</span>`).join('')}
          </div>
        </div>
      </div>
    `;
    el.querySelectorAll<HTMLDivElement>('[data-h]').forEach((cell) => {
      cell.addEventListener('click', () => toggle(Number(cell.dataset.h)));
    });
    return;
  }

  // Donut (SVG)
  const cx = 110, cy = 110, rOuter = 100, rInner = 60;
  const segs = schedule.map((on, h) => {
    const a1 = (h / 24) * Math.PI * 2 - Math.PI / 2;
    const a2 = ((h + 1) / 24) * Math.PI * 2 - Math.PI / 2;
    const x1o = cx + rOuter * Math.cos(a1), y1o = cy + rOuter * Math.sin(a1);
    const x2o = cx + rOuter * Math.cos(a2), y2o = cy + rOuter * Math.sin(a2);
    const x1i = cx + rInner * Math.cos(a2), y1i = cy + rInner * Math.sin(a2);
    const x2i = cx + rInner * Math.cos(a1), y2i = cy + rInner * Math.sin(a1);
    const path = `M ${x1o} ${y1o} A ${rOuter} ${rOuter} 0 0 1 ${x2o} ${y2o} L ${x1i} ${y1i} A ${rInner} ${rInner} 0 0 0 ${x2i} ${y2i} Z`;
    return { h, on, path };
  });

  // Marcador da hora atual
  const aNow = (currentHour / 24) * Math.PI * 2 - Math.PI / 2;
  const xn1 = cx + (rInner - 4) * Math.cos(aNow), yn1 = cy + (rInner - 4) * Math.sin(aNow);
  const xn2 = cx + (rOuter + 6) * Math.cos(aNow), yn2 = cy + (rOuter + 6) * Math.sin(aNow);

  // Labels cardinais
  const labels = [0, 6, 12, 18].map((h) => {
    const a = (h / 24) * Math.PI * 2 - Math.PI / 2;
    return {
      h,
      x: cx + (rOuter + 14) * Math.cos(a),
      y: cy + (rOuter + 14) * Math.sin(a),
    };
  });

  el.innerHTML = `
    <div class="schedule">
      <div class="schedule-donut">
        <svg class="donut-svg" viewBox="0 0 220 220">
          ${segs.map((s) => `
            <path d="${s.path}" class="donut-cell" data-h="${s.h}"
                  fill="${s.on ? 'var(--accent)' : 'var(--surface)'}"
                  stroke="var(--border)" stroke-width="1" />
          `).join('')}
          <line x1="${xn1}" y1="${yn1}" x2="${xn2}" y2="${yn2}"
                stroke="var(--fg)" stroke-width="2" stroke-linecap="round" />
          ${labels.map((l) =>
            `<text x="${l.x}" y="${l.y}" class="donut-label" text-anchor="middle"
                   dominant-baseline="middle">${l.h.toString().padStart(2, '0')}h</text>`
          ).join('')}
          <text x="${cx}" y="${cy - 6}" class="donut-center">${schedule.filter(Boolean).length}h/24</text>
          <text x="${cx}" y="${cy + 12}" class="donut-center-sub">permitido</text>
        </svg>
      </div>
    </div>
  `;
  el.querySelectorAll<SVGPathElement>('[data-h]').forEach((cell) => {
    cell.addEventListener('click', () => toggle(Number(cell.dataset.h)));
  });
}
