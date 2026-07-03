// =============================================
// Gráficos SVG (sem libs) · porte do charts.jsx do handoff
// =============================================
//
// Todos devolvem string de SVG/HTML pronta pra innerHTML. Interatividade
// (schedule, slider) é ligada por quem renderiza via data-* atributos.

import { icon } from '../icons';
import type { Estado } from '../types';

function smoothPath(pts: Array<[number, number]>): string {
  if (pts.length < 2) return '';
  let d = `M ${pts[0][0]},${pts[0][1]}`;
  for (let i = 0; i < pts.length - 1; i++) {
    const [x0, y0] = pts[i];
    const [x1, y1] = pts[i + 1];
    const cx = (x0 + x1) / 2;
    d += ` C ${cx},${y0} ${cx},${y1} ${x1},${y1}`;
  }
  return d;
}

/** Mini barras verticais nos cards de tomada (78×38). */
export function miniBars(data: number[], estado: Estado): string {
  const w = 78, h = 38, n = 7;
  const color = estado === 'ON' ? 'var(--green)' : estado === 'STD' ? 'var(--amber)' : 'var(--lav-bar)';
  const src = data && data.length ? data : new Array(n).fill(0);
  const step = Math.max(1, Math.floor(src.length / n));
  const vals: number[] = [];
  for (let i = 0; i < n; i++) vals.push(src[Math.min(src.length - 1, i * step)] || 0);
  const max = Math.max(...vals, 1);
  const bw = (w - (n - 1) * 5) / n;
  const bars = vals
    .map((v, i) => {
      const bh = estado === 'OFF' ? 4 : Math.max(4, (v / max) * h);
      const op = estado === 'OFF' ? 0.5 : 0.5 + 0.5 * (v / max);
      return `<rect x="${i * (bw + 5)}" y="${h - bh}" width="${bw}" height="${bh}" rx="2.5" fill="${color}" opacity="${op.toFixed(2)}"/>`;
    })
    .join('');
  return `<svg class="tcard__bars" viewBox="0 0 ${w} ${h}" preserveAspectRatio="none" aria-hidden="true">${bars}</svg>`;
}

/** Anel de eficiência (0–100). `null` = sem dados → anel vazio com "—". */
export function efficiencyRing(value: number | null): string {
  const size = 150, sw = 13, r = (size - sw) / 2, c = 2 * Math.PI * r;
  const semDados = value === null;
  const off = semDados ? c : c * (1 - value / 100); // sem dados => anel vazio
  return `
    <div class="eff__ring">
      <svg width="${size}" height="${size}" viewBox="0 0 ${size} ${size}">
        <circle cx="${size / 2}" cy="${size / 2}" r="${r + sw / 2 + 2}" fill="none" stroke="var(--lav)" stroke-width="2"/>
        <circle cx="${size / 2}" cy="${size / 2}" r="${r}" fill="none" stroke="var(--lav-track)" stroke-width="${sw}"/>
        <circle cx="${size / 2}" cy="${size / 2}" r="${r}" fill="none" stroke="var(--green)" stroke-width="${sw}"
          stroke-linecap="round" stroke-dasharray="${c}" stroke-dashoffset="${off}"
          transform="rotate(-90 ${size / 2} ${size / 2})" style="transition:stroke-dashoffset .8s cubic-bezier(.2,.7,.2,1)"/>
      </svg>
      <div class="eff__val tnum">${semDados ? '—' : Math.round(value)}</div>
    </div>`;
}

/** Barra de capacidade (trilho lavanda + marcador de aviso 85%). */
export function capacityBar(total: number, pMax: number): string {
  const pct = Math.min(1, pMax > 0 ? total / pMax : 0);
  const over = pct >= 0.85;
  const color = pct < 0.7 ? 'var(--green-deep)' : pct < 0.85 ? 'var(--amber)' : pct < 0.95 ? 'var(--amber-ink)' : 'var(--red)';
  const alert = over
    ? `<div class="cap__alert">${icon('warn', 15, 2.2)}Perto do teto elétrico — novos comandos manuais podem ser recusados para manter a casa segura.</div>`
    : '';
  return `
    <div class="cap">
      <div class="cap__row">
        <span class="cap__label">Uso da capacidade</span>
        <span class="cap__pmax tnum">P_max: <b>${pMax.toLocaleString('pt-BR')} W</b></span>
      </div>
      <div class="cap__track">
        <div class="cap__fill" style="width:${Math.max(2, pct * 100).toFixed(1)}%;background:${color}"></div>
        <div class="cap__mark" style="left:85%"></div>
      </div>
      <div class="cap__ends">
        <span class="l tnum">0 W</span>
        <span class="warn">Limite de aviso (85%)</span>
        <span class="r tnum">${pMax.toLocaleString('pt-BR')} W</span>
      </div>
      ${alert}
    </div>`;
}

const STATE_LINE: Record<Estado, string> = {
  ON: 'var(--green-line)',
  STD: 'var(--amber)',
  OFF: '#9aa1b1',
};

/** Gráfico de consumo (linha suave + área, eixo em kW). */
export function consumptionChart(data: number[], estado: Estado): string {
  const serie = data.length ? data : [0, 0];
  const color = STATE_LINE[estado];
  const w = 760, h = 230, padL = 30, padR = 8, padT = 18, padB = 26;
  const max = Math.max(...serie, 200);
  const niceMax = Math.ceil(max / 500) * 500;
  const innerW = w - padL - padR, innerH = h - padT - padB;
  const pts: Array<[number, number]> = serie.map((v, i) => [
    padL + (serie.length === 1 ? 0 : (i / (serie.length - 1)) * innerW),
    padT + innerH - (v / niceMax) * innerH,
  ]);
  const line = smoothPath(pts);
  const last = pts[pts.length - 1];
  const area = `${line} L ${last[0]},${padT + innerH} L ${pts[0][0]},${padT + innerH} Z`;
  const ticks = [0, 0.25, 0.5, 0.75, 1];
  const labels = ['12:00', '18:00', '00:00', '06:00', 'Agora'];
  const grid = ticks
    .map((f) => {
      const y = padT + innerH - f * innerH;
      const lab = `${((niceMax * f) / 1000).toFixed(f === 0 ? 0 : 1)}${f === 0 ? 'kW' : ''}`;
      return `<line x1="${padL}" y1="${y}" x2="${w - padR}" y2="${y}" stroke="var(--line)" stroke-width="1" stroke-dasharray="${f === 0 ? '0' : '3 5'}"/><text class="chart-axis" x="4" y="${y + 3.5}">${lab}</text>`;
    })
    .join('');
  const xlabels = labels
    .map((lb, i) => {
      const x = padL + (i / (labels.length - 1)) * innerW;
      const anchor = i === 0 ? 'start' : i === labels.length - 1 ? 'end' : 'middle';
      return `<text class="chart-axis" x="${x}" y="${h - 7}" text-anchor="${anchor}">${lb}</text>`;
    })
    .join('');
  return `
    <svg class="chart" viewBox="0 0 ${w} ${h}" role="img" aria-label="Histórico de consumo">
      <defs>
        <linearGradient id="cfill" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stop-color="${color}" stop-opacity="0.22"/>
          <stop offset="100%" stop-color="${color}" stop-opacity="0"/>
        </linearGradient>
      </defs>
      ${grid}
      <path d="${area}" fill="url(#cfill)"/>
      <path d="${line}" fill="none" stroke="${color}" stroke-width="2.6" stroke-linejoin="round" stroke-linecap="round"/>
      <circle cx="${last[0]}" cy="${last[1]}" r="4.5" fill="${color}" stroke="#fff" stroke-width="2"/>
      ${xlabels}
    </svg>`;
}

/** Probabilidade de uso por hora (24 barras; hora atual em verde claro). */
export function probabilityChart(pi: number[], hourNow: number): string {
  const w = 420, h = 170, padT = 10, padB = 24, n = 24, gap = 3;
  const bw = (w - gap * (n - 1)) / n, innerH = h - padT - padB;
  const max = Math.max(...pi, 0.01);
  const bars = pi
    .map((v, i) => {
      const ratio = v / max;
      const bh = Math.max(2, ratio * innerH);
      const x = i * (bw + gap), y = padT + innerH - bh;
      const peak = ratio > 0.6;
      const fill = i === hourNow ? 'var(--green)' : peak ? 'var(--green-deep)' : 'var(--lav-bar)';
      return `<rect x="${x}" y="${y}" width="${bw}" height="${bh}" rx="2" fill="${fill}"/>`;
    })
    .join('');
  const axis = [0, 12, 23]
    .map((hh) => {
      const x = hh * (bw + gap) + bw / 2;
      const anchor = hh === 0 ? 'start' : hh === 23 ? 'end' : 'middle';
      return `<text class="chart-axis" x="${x}" y="${h - 7}" text-anchor="${anchor}">${hh}h</text>`;
    })
    .join('');
  return `<svg class="chart" viewBox="0 0 ${w} ${h}" role="img" aria-label="Probabilidade de uso por hora">${bars}${axis}</svg>`;
}

/** Agendamento operacional 24h (barra pintável). Botões com data-h. */
export function scheduleBar(schedule: boolean[], disabled = false): string {
  const segs = schedule
    .map((on, h) => {
      const hh = String(h).padStart(2, '0');
      return `<button class="sched__seg${on ? ' sched__seg--on' : ''}" data-h="${h}" ${disabled ? 'disabled' : ''} title="${hh}:00 — ${on ? 'permitido' : 'bloqueado'}" aria-label="${hh}h ${on ? 'permitido' : 'bloqueado'}"></button>`;
    })
    .join('');
  const ticks = Array.from({ length: 13 }, (_, i) => `<div class="sched__tick tnum">${String(i * 2).padStart(2, '0')}</div>`).join('');
  return `
    <div class="sched">
      <div class="sched__head">
        <div class="sched__legend">
          <span><i class="sched__sw" style="background:var(--green-deep)"></i>Permitido</span>
          <span><i class="sched__sw" style="background:var(--lav-track)"></i>Bloqueado</span>
        </div>
      </div>
      <div class="sched__bar">${segs}</div>
      <div class="sched__ticks">${ticks}</div>
    </div>`;
}

/** Slider de potência (0..max). Input com data-role="power-slider". */
export function powerSlider(value: number, max: number, label = 'Consumo máximo (Watts)'): string {
  const pct = max > 0 ? (value / max) * 100 : 0;
  return `
    <div class="pslider">
      <div class="slider-row">
        <span class="slider-label">${label}</span>
        <span class="slider-val tnum" data-role="power-val">${value.toLocaleString('pt-BR')} W</span>
      </div>
      <input class="rng" type="range" min="0" max="${max}" step="50" value="${value}" data-role="power-slider"
        style="background:linear-gradient(to right, var(--green) 0%, var(--green) ${pct}%, var(--lav-track) ${pct}%, var(--lav-track) 100%)"/>
      <div class="rng-ends tnum"><span>0 W</span><span>${max.toLocaleString('pt-BR')} W</span></div>
    </div>`;
}
