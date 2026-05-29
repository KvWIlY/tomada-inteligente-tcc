// =============================================
// Grafico de potencia ao longo do tempo (Chart.js v4)
// =============================================
//
// Substitui o Recharts do mockup. Features:
// - linha suave de potencia com area gradient
// - bandas de fundo coloridas por estado (plugin custom beforeDraw)
// - linha tracejada no P_limiar
// - tooltip com hora + potencia + estado
// - 3 janelas: 1h, 24h, 7d (replica 7d a partir de 24h)

import {
  Chart,
  type ChartDataset,
  type ChartOptions,
  CategoryScale,
  Filler,
  LinearScale,
  LineController,
  LineElement,
  PointElement,
  Tooltip,
} from 'chart.js';
import { ICONS } from '../icons';
import type { Estado, Medicao } from '../types';
import { fmtTime } from '../utils';

Chart.register(
  CategoryScale, LinearScale, LineController, LineElement, PointElement, Filler, Tooltip,
);

// Canvas nao entende CSS custom properties (var(...)). Esse helper le o valor
// REAL de --accent-h em runtime e monta uma cor oklch() literal aceita pelo canvas.
function accentColor(alpha?: number): string {
  const h = getComputedStyle(document.documentElement).getPropertyValue('--accent-h').trim() || '285';
  return alpha !== undefined ? `oklch(0.58 0.18 ${h} / ${alpha})` : `oklch(0.58 0.18 ${h})`;
}

export type ChartWindow = '1h' | '24h' | '7d';

export interface ChartProps {
  medicoes: Medicao[];
  pLimiar: number;
  win: ChartWindow;
  onChangeWin: (w: ChartWindow) => void;
}

let chartInstance: Chart<'line'> | null = null;

const STATE_COLORS: Record<Estado, string> = {
  ON:  'oklch(0.68 0.16 155 / 0.18)',
  STD: 'oklch(0.78 0.13 75 / 0.20)',
  OFF: 'oklch(0.65 0.18 25 / 0.18)',
};

interface Ponto {
  ts: number;       // unix sec
  p: number;
  estado: Estado;
}

function janelaDe(medicoes: Medicao[], win: ChartWindow): Ponto[] {
  if (win === '1h') {
    return medicoes.slice(-60).map((m) => ({ ts: m.timestamp, p: m.p_medida, estado: m.estado }));
  }
  if (win === '24h') {
    return medicoes.map((m) => ({ ts: m.timestamp, p: m.p_medida, estado: m.estado }));
  }
  // 7d: so temos 24h do dispositivo, replicamos 7 vezes com variacao deterministica por dia
  const out: Ponto[] = [];
  const agora = Math.floor(Date.now() / 1000);
  for (let d = 6; d >= 0; d--) {
    for (let h = 0; h < 24; h++) {
      const sample = medicoes.filter((m) => new Date(m.timestamp * 1000).getHours() === h);
      const avg =
        sample.length > 0 ? sample.reduce((s, m) => s + m.p_medida, 0) / sample.length : 0;
      // Variacao deterministica por dia (sem Math.random): d=0 hoje, d=6 mais antigo
      const scale = 0.88 + ((d * 3 + h) % 7) * 0.04; // range 0.88..1.12
      const p = avg * scale;
      const estado: Estado = avg > 8 ? 'ON' : avg > 1 ? 'STD' : 'OFF';
      out.push({
        ts: agora - d * 24 * 3600 + h * 3600,
        p,
        estado,
      });
    }
  }
  return out;
}

/** Plugin custom: pinta retangulos verticais por estado. */
const stateBandsPlugin = {
  id: 'stateBands',
  beforeDatasetsDraw(chart: Chart) {
    const { ctx, chartArea, scales } = chart;
    if (!chartArea || !scales.x) return;
    const dataset = chart.data.datasets[0];
    const estados = (dataset as ChartDataset<'line'> & { _estados?: Estado[] })._estados;
    if (!estados) return;

    let i = 0;
    while (i < estados.length) {
      const cur = estados[i];
      let j = i;
      while (j < estados.length && estados[j] === cur) j++;
      const xStart = scales.x.getPixelForValue(i);
      const xEnd = scales.x.getPixelForValue(j - 1);
      ctx.save();
      ctx.fillStyle = STATE_COLORS[cur];
      ctx.fillRect(xStart, chartArea.top, xEnd - xStart, chartArea.bottom - chartArea.top);
      ctx.restore();
      i = j;
    }
  },
};

/** Plugin custom: linha tracejada horizontal no P_limiar. */
const pLimiarPlugin = {
  id: 'pLimiarLine',
  afterDatasetsDraw(chart: Chart, _args: unknown, opts: { value?: number }) {
    if (!opts || typeof opts.value !== 'number') return;
    const { ctx, chartArea, scales } = chart;
    if (!chartArea || !scales.y) return;
    const y = scales.y.getPixelForValue(opts.value);
    if (y < chartArea.top || y > chartArea.bottom) return;
    const cor = accentColor();
    ctx.save();
    ctx.strokeStyle = cor;
    ctx.lineWidth = 1.5;
    ctx.setLineDash([4, 4]);
    ctx.beginPath();
    ctx.moveTo(chartArea.left, y);
    ctx.lineTo(chartArea.right, y);
    ctx.stroke();
    // Label
    ctx.fillStyle = cor;
    ctx.font = '11px "JetBrains Mono", monospace';
    ctx.fillText(`P_limiar ${opts.value}W`, chartArea.right - 90, y - 4);
    ctx.restore();
  },
};

Chart.register(stateBandsPlugin, pLimiarPlugin);

export function renderChart(el: HTMLElement, p: ChartProps): void {
  const tabs: Array<[ChartWindow, string]> = [
    ['1h',  'Ultima hora'],
    ['24h', '24 horas'],
    ['7d',  '7 dias (est.)'],
  ];

  el.innerHTML = `
    <section class="section">
      <div class="section-head">
        <div>
          <h2 class="section-title">${ICONS.bolt} Potencia ao longo do tempo</h2>
          <p class="section-sub">Linha = p_medida · faixas de fundo = estado decidido pelo solver</p>
        </div>
        <div class="tabs">
          ${tabs.map(([k, l]) => `<button class="tab ${k === p.win ? 'active' : ''}" data-win="${k}">${l}</button>`).join('')}
        </div>
      </div>
      <div style="height:320px"><canvas id="power-chart"></canvas></div>
      <div class="chart-legend">
        <span class="legend-swatch"><i style="background:${STATE_COLORS.ON};border:1px solid var(--on)"></i> ON · uso ativo</span>
        <span class="legend-swatch"><i style="background:${STATE_COLORS.STD};border:1px solid var(--std)"></i> STD · standby</span>
        <span class="legend-swatch"><i style="background:${STATE_COLORS.OFF};border:1px solid var(--off)"></i> OFF · rele aberto</span>
        <span class="legend-swatch"><i style="background:transparent;border:1px dashed var(--accent)"></i> P_limiar</span>
      </div>
    </section>
  `;

  el.querySelectorAll<HTMLButtonElement>('[data-win]').forEach((btn) => {
    btn.addEventListener('click', () => {
      const w = btn.dataset.win as ChartWindow;
      if (w) p.onChangeWin(w);
    });
  });

  // Constroi o chart
  const pontos = janelaDe(p.medicoes, p.win);
  const labels = pontos.map((pt, i) => {
    if (p.win === '7d') {
      // Mostra dia da semana so ao meio-dia
      const d = new Date(pt.ts * 1000);
      if (d.getHours() === 12) {
        const dias = ['dom', 'seg', 'ter', 'qua', 'qui', 'sex', 'sab'];
        return dias[d.getDay()];
      }
      return '';
    }
    // 1h ou 24h: mostra HH:MM em pontos espacados
    const total = pontos.length;
    const stride = Math.max(1, Math.floor(total / 8));
    return i % stride === 0 ? fmtTime(pt.ts) : '';
  });
  const dados = pontos.map((pt) => pt.p);
  const estados = pontos.map((pt) => pt.estado);

  const canvas = el.querySelector<HTMLCanvasElement>('#power-chart');
  if (!canvas) return;
  const ctx = canvas.getContext('2d');
  if (!ctx) return;

  // Gradient de area
  const gradient = ctx.createLinearGradient(0, 0, 0, 320);
  gradient.addColorStop(0, accentColor(0.35));
  gradient.addColorStop(1, accentColor(0.02));

  if (chartInstance) {
    chartInstance.destroy();
    chartInstance = null;
  }

  const dataset: ChartDataset<'line'> & { _estados?: Estado[] } = {
    label: 'Potencia',
    data: dados,
    borderColor: accentColor(),
    borderWidth: 1.8,
    backgroundColor: gradient,
    fill: true,
    tension: 0.35,
    pointRadius: 0,
    pointHoverRadius: 4,
    _estados: estados,
  };

  const options: ChartOptions<'line'> = {
    responsive: true,
    maintainAspectRatio: false,
    animation: false,
    interaction: { mode: 'index', intersect: false },
    plugins: {
      legend: { display: false },
      tooltip: {
        callbacks: {
          title: (items) => {
            const idx = items[0].dataIndex;
            const pt = pontos[idx];
            return new Date(pt.ts * 1000).toLocaleString('pt-BR', {
              hour: '2-digit', minute: '2-digit', day: '2-digit', month: '2-digit',
            });
          },
          label: (item) => {
            const idx = item.dataIndex;
            const pt = pontos[idx];
            return [`Potencia: ${pt.p.toFixed(1)} W`, `Estado: ${pt.estado}`];
          },
        },
      },
      // @ts-expect-error -- nosso plugin custom (Chart.js types nao incluem)
      pLimiarLine: { value: p.pLimiar },
    },
    scales: {
      x: {
        ticks: {
          color: 'oklch(0.45 0.012 270)',
          maxRotation: 0,
          autoSkip: false,
          callback: (_val, i) => labels[i] ?? '',
        },
        grid: { display: false },
      },
      y: {
        beginAtZero: true,
        ticks: {
          color: 'oklch(0.45 0.012 270)',
          callback: (v) => `${v}W`,
        },
        grid: { color: 'oklch(0.92 0.006 270 / 0.5)' },
      },
    },
  };

  chartInstance = new Chart<'line'>(canvas, {
    type: 'line',
    data: { labels, datasets: [dataset] },
    options,
  });
}
