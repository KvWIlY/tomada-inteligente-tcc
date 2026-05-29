// =============================================
// Tweaks panel (canto inferior direito)
// =============================================
//
// Versao simplificada do tweaks-panel.jsx do mockup -- sem drag, sem postMessage,
// sem edit mode. So as opcoes funcionais que o usuario quer mexer.

import { ICONS } from '../icons';
import type {
  Accent,
  Density,
  EstadoOverride,
  ScheduleStyle,
  Theme,
} from '../types';

export interface TweaksProps {
  theme: Theme;
  accent: Accent;
  density: Density;
  scheduleStyle: ScheduleStyle;
  estadoOverride: EstadoOverride;

  onChangeTheme: (t: Theme) => void;
  onChangeAccent: (a: Accent) => void;
  onChangeDensity: (d: Density) => void;
  onChangeScheduleStyle: (s: ScheduleStyle) => void;
  onChangeEstadoOverride: (e: EstadoOverride) => void;
}

const ACCENT_HUES: Record<Accent, number> = {
  violet: 285, indigo: 260, teal: 195, orange: 45, pink: 350,
};

let panelOpen = false;

export function renderTweaks(el: HTMLElement, p: TweaksProps): void {
  el.innerHTML = `
    <button class="tweaks-toggle" data-toggle title="Configuracoes">
      ${ICONS.cog}
    </button>
    <div class="tweaks-card ${panelOpen ? 'open' : ''}">
      <div class="tweaks-head">
        <div class="tweaks-title">Configuracoes</div>
        <button class="tweaks-close" data-close>×</button>
      </div>

      <div class="tweaks-section">
        <div class="tweaks-label">Tema</div>
        <div class="tweaks-radio">
          ${radioBtn('theme', 'light', p.theme === 'light', 'Claro')}
          ${radioBtn('theme', 'dark', p.theme === 'dark', 'Escuro')}
        </div>
      </div>

      <div class="tweaks-section">
        <div class="tweaks-label">Cor de acento</div>
        <div class="tweaks-colors">
          ${(Object.keys(ACCENT_HUES) as Accent[])
            .map((a) => `
              <button class="color-swatch ${p.accent === a ? 'active' : ''}"
                      data-accent="${a}" title="${a}"
                      style="background:oklch(0.58 0.18 ${ACCENT_HUES[a]})"></button>
            `).join('')}
        </div>
      </div>

      <div class="tweaks-section">
        <div class="tweaks-label">Densidade</div>
        <div class="tweaks-radio">
          ${radioBtn('density', 'compact', p.density === 'compact', 'Compacto')}
          ${radioBtn('density', 'comfortable', p.density === 'comfortable', 'Medio')}
          ${radioBtn('density', 'spacious', p.density === 'spacious', 'Amplo')}
        </div>
      </div>

      <div class="tweaks-section">
        <div class="tweaks-label">Estilo do schedule</div>
        <div class="tweaks-radio">
          ${radioBtn('schedule', 'grid', p.scheduleStyle === 'grid', 'Grid')}
          ${radioBtn('schedule', 'donut', p.scheduleStyle === 'donut', 'Donut')}
          ${radioBtn('schedule', 'bar', p.scheduleStyle === 'bar', 'Barra')}
        </div>
      </div>

      <div class="tweaks-section">
        <div class="tweaks-label">Estado demo (so afeta UI)</div>
        <div class="tweaks-radio">
          ${radioBtn('demo', 'auto', p.estadoOverride === 'auto', 'Auto')}
          ${radioBtn('demo', 'ON',  p.estadoOverride === 'ON',  'ON')}
          ${radioBtn('demo', 'STD', p.estadoOverride === 'STD', 'STD')}
          ${radioBtn('demo', 'OFF', p.estadoOverride === 'OFF', 'OFF')}
        </div>
      </div>
    </div>
  `;

  // Toggle painel
  el.querySelector<HTMLButtonElement>('[data-toggle]')?.addEventListener('click', () => {
    panelOpen = !panelOpen;
    renderTweaks(el, p);
  });
  el.querySelector<HTMLButtonElement>('[data-close]')?.addEventListener('click', () => {
    panelOpen = false;
    renderTweaks(el, p);
  });

  // Radios
  el.querySelectorAll<HTMLButtonElement>('[data-radio]').forEach((btn) => {
    btn.addEventListener('click', () => {
      const group = btn.dataset.radio;
      const value = btn.dataset.value;
      if (!group || !value) return;
      switch (group) {
        case 'theme':    p.onChangeTheme(value as Theme); break;
        case 'density':  p.onChangeDensity(value as Density); break;
        case 'schedule': p.onChangeScheduleStyle(value as ScheduleStyle); break;
        case 'demo':     p.onChangeEstadoOverride(value as EstadoOverride); break;
      }
    });
  });

  // Cores
  el.querySelectorAll<HTMLButtonElement>('[data-accent]').forEach((btn) => {
    btn.addEventListener('click', () => p.onChangeAccent(btn.dataset.accent as Accent));
  });
}

function radioBtn(group: string, value: string, active: boolean, label: string): string {
  return `<button class="tweaks-radio-btn ${active ? 'active' : ''}"
                  data-radio="${group}" data-value="${value}">${label}</button>`;
}
