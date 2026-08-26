// =============================================
// Graficos — Chart.js pelo wrapper do Basecoat + SVG onde ele nao cabe
// =============================================
//
// `consumptionChart` e o unico que virou Chart.js: e o grafico grande, com
// eixo, tooltip e leitura de valor. `miniBars` e `efficiencyRing` seguem SVG
// por decisao registrada (T051/CHK001) — sao 24 barras estaticas por cartao e
// um anel de um numero; instanciar um Chart.js para cada um custaria um canvas
// e um ciclo de vida por tomada para desenhar o que uma string ja desenha.
//
// `probabilityChart` e `scheduleBar` foram REMOVIDOS (FR-009, FR-009a) junto
// com os cartoes que os usavam.

import { icon } from '../icons';
import type { Estado, Medicao } from '../types';
import { fmt2 } from '../derive';

/** Cor da serie por estado da tomada. Tokens, nunca hexadecimal solto. */
const STATE_LINE: Record<Estado, string> = {
  ON: 'var(--green-line)',
  STD: 'var(--amber-line)',
  OFF: 'var(--ink-faint)',
};

// =============================================
// Escala adaptativa (FR-012)
// =============================================

/**
 * Teto "redondo" do eixo e o passo entre as marcas.
 *
 * O piso fixo de 500 W que existia antes de 2026-08-24 achatava toda carga de
 * standby contra o eixo: uma tomada de 3 W virava linha reta no zero —
 * exatamente o consumo que este trabalho existe para tornar visivel (SC-005).
 */
export function escala(valores: number[]): { max: number; passo: number; emKW: boolean } {
  const pico = Math.max(...valores, 1);
  const passo = pico <= 20 ? 5 : pico <= 50 ? 10 : pico <= 100 ? 25 : pico <= 500 ? 100 : 500;
  const max = Math.max(passo, Math.ceil(pico / passo) * passo);
  return { max, passo, emKW: max >= 1000 };
}

function rotuloEixo(w: number, emKW: boolean): string {
  return emKW ? `${(w / 1000).toFixed(1)} kW` : `${Math.round(w)} W`;
}

// =============================================
// Historico de consumo (Chart.js via Basecoat)
// =============================================

/**
 * Carimbos de tempo alinhados 1:1 com o que `derive.seriesFromMedicoes(m, n)`
 * devolve.
 *
 * A reamostragem vive em `derive.ts`, que esta FORA do contrato desta feature e
 * nao pode ser alterada. Como o eixo horizontal precisa dos timestamps dos
 * MESMOS baldes (FR-019), o indice de cada balde e recalculado aqui com a
 * mesma aritmetica. Se a de la mudar, esta precisa mudar junto — e o motivo de
 * as duas estarem descritas em comentario nos dois arquivos.
 */
export function carimbosDaSerie(medicoes: Medicao[], n: number): number[] {
  if (medicoes.length === 0) return [];
  if (medicoes.length <= n) return medicoes.map((m) => m.timestamp);
  const bucket = medicoes.length / n;
  return Array.from({ length: n }, (_, i) => medicoes[Math.floor(i * bucket)].timestamp);
}

/**
 * Formato do rotulo derivado do SPAN REAL da serie, nao do nome da janela
 * (FR-019). Uma instalacao nova com dois dias de historico, aberta em "7 dias",
 * imprimiria a mesma data sete vezes seguidas se o formato viesse do seletor.
 */
function formatoDe(carimbos: number[]): 'hora' | 'dia' {
  if (carimbos.length < 2) return 'hora';
  const span = carimbos[carimbos.length - 1] - carimbos[0];
  return span <= 48 * 3600 ? 'hora' : 'dia';
}

function rotuloTempo(unixSec: number, formato: 'hora' | 'dia'): string {
  const d = new Date(unixSec * 1000);
  return formato === 'hora'
    ? d.toLocaleTimeString('pt-BR', { hour: '2-digit', minute: '2-digit' })
    : d.toLocaleDateString('pt-BR', { day: '2-digit', month: '2-digit' });
}

/** Descricao textual equivalente do grafico, para leitor de tela (US4 AS3). */
function descricaoSerie(valores: number[], carimbos: number[]): string {
  const fmt = formatoDe(carimbos);
  const media = valores.reduce((s, v) => s + v, 0) / valores.length;
  const pico = Math.max(...valores);
  const iPico = valores.indexOf(pico);
  const de = carimbos.length ? rotuloTempo(carimbos[0], fmt) : '';
  const ate = carimbos.length ? rotuloTempo(carimbos[carimbos.length - 1], fmt) : '';
  return (
    `Histórico de potência ativa de ${de} a ${ate}, com ${valores.length} pontos. ` +
    `Média ${fmt2(media)} watts, mínimo ${fmt2(Math.min(...valores))} watts, ` +
    `pico ${fmt2(pico)} watts${carimbos[iPico] ? ` às ${rotuloTempo(carimbos[iPico], fmt)}` : ''}.`
  );
}

/**
 * Marcacao do cartao de historico. O `<canvas>` so vira grafico em
 * `montarConsumo`, que roda depois do `innerHTML`.
 *
 * FR-020: sem runtime de grafico (`window.Chart` ausente, ou o wrapper do
 * Basecoat nao carregou), o cartao cai numa leitura textual dos mesmos numeros
 * em vez de deixar um retangulo vazio no meio da tela.
 */
export function consumptionChart(valores: number[], carimbos: number[]): string {
  const descricao = descricaoSerie(valores, carimbos);
  if (!temRuntimeDeGrafico()) {
    const { emKW } = escala(valores);
    const ultimo = valores[valores.length - 1] ?? 0;
    return `
      <div class="alert" data-variant="destructive" role="status">
        ${icon('warn', 16)}
        <h3>Gráfico indisponível</h3>
        <section>
          <p>O runtime de gráficos não carregou. Os números continuam abaixo, medidos normalmente.</p>
          <ul class="tnum">
            <li>Leitura mais recente: <strong>${rotuloEixo(ultimo, emKW)}</strong></li>
            <li>Média da janela: <strong>${rotuloEixo(valores.reduce((s, v) => s + v, 0) / valores.length, emKW)}</strong></li>
            <li>Pico da janela: <strong>${rotuloEixo(Math.max(...valores), emKW)}</strong></li>
          </ul>
        </section>
      </div>`;
  }
  return `
    <figure class="m-0 h-[230px] w-full" data-role="consumo">
      <canvas class="chart h-full w-full" role="img" aria-label="${descricao.replace(/"/g, '&quot;')}"></canvas>
    </figure>
    <p class="sr-only">${descricao}</p>`;
}

function temRuntimeDeGrafico(): boolean {
  return typeof window !== 'undefined' && Boolean(window.Chart) && Boolean(window.basecoat?.chart);
}

/** Instancia o grafico de consumo dentro do `[data-role="consumo"]` de `root`. */
export function montarConsumo(
  root: HTMLElement,
  valores: number[],
  carimbos: number[],
  estado: Estado,
): void {
  const alvo = root.querySelector<HTMLCanvasElement>('[data-role="consumo"] canvas');
  if (!alvo || !temRuntimeDeGrafico()) return;

  const { max, passo, emKW } = escala(valores);
  const fmt = formatoDe(carimbos);
  const linhas = valores.map((v, i) => ({
    t: carimbos[i] !== undefined ? rotuloTempo(carimbos[i], fmt) : '',
    w: v,
  }));

  try {
    window.basecoat!.chart(alvo, {
      type: 'line',
      labelKey: 't',
      data: linhas,
      series: {
        w: {
          label: 'Potência ativa',
          color: STATE_LINE[estado],
          surface: 'gradient',
          dataset: { tension: 0.35, borderWidth: 2.6, pointRadius: 0, pointHitRadius: 12, fill: true },
        },
      },
      options: {
        scales: {
          y: {
            min: 0,
            max,
            ticks: { display: true, stepSize: passo, callback: (v) => rotuloEixo(Number(v), emKW) },
          },
          // Sem `maxTicksLimit` o eixo de 7 dias imprimiria 168 datas em cima
          // umas das outras; os rotulos vem dos carimbos reais (FR-019).
          x: { ticks: { maxTicksLimit: 6, autoSkip: true, maxRotation: 0 } },
        },
      },
    });
  } catch (err) {
    // FR-020: um grafico que explode nao pode levar a tela junto.
    console.error('[charts] montarConsumo:', err);
  }
}

// =============================================
// Sparkline do cartao de tomada (SVG — T051)
// =============================================

/** Mini barras verticais nos cards de tomada (78x38). */
export function miniBars(data: number[], estado: Estado): string {
  const w = 78, h = 38, n = 7;
  const color = estado === 'ON' ? 'var(--green)' : estado === 'STD' ? 'var(--amber-line)' : 'var(--lav-bar)';
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
  return `<svg class="h-[38px] w-[78px] shrink-0" viewBox="0 0 ${w} ${h}" preserveAspectRatio="none" aria-hidden="true">${bars}</svg>`;
}

// =============================================
// Anel de eficiencia (SVG — o valor e um numero so)
// =============================================

/** Anel de eficiência (0–100). `null` = sem dados → anel vazio com "—". */
export function efficiencyRing(value: number | null): string {
  const size = 150, sw = 13, r = (size - sw) / 2, c = 2 * Math.PI * r;
  const semDados = value === null;
  const off = semDados ? c : c * (1 - value / 100); // sem dados => anel vazio
  const rotulo = semDados
    ? 'Pontuação de eficiência indisponível: nenhuma tomada com standby calibrado.'
    : `Pontuação de eficiência: ${Math.round(value)} de 100.`;
  return `
    <div class="relative my-1.5 size-[150px]">
      <svg width="${size}" height="${size}" viewBox="0 0 ${size} ${size}" role="img" aria-label="${rotulo}">
        <circle cx="${size / 2}" cy="${size / 2}" r="${r + sw / 2 + 2}" fill="none" stroke="var(--lav)" stroke-width="2"/>
        <circle cx="${size / 2}" cy="${size / 2}" r="${r}" fill="none" stroke="var(--lav-track)" stroke-width="${sw}"/>
        <circle cx="${size / 2}" cy="${size / 2}" r="${r}" fill="none" stroke="var(--green)" stroke-width="${sw}"
          stroke-linecap="round" stroke-dasharray="${c}" stroke-dashoffset="${off}"
          transform="rotate(-90 ${size / 2} ${size / 2})" style="transition:stroke-dashoffset .8s cubic-bezier(.2,.7,.2,1)"/>
      </svg>
      <div class="tnum absolute inset-0 grid place-items-center text-[40px] font-extrabold tracking-tighter">${semDados ? '—' : Math.round(value)}</div>
    </div>`;
}

// =============================================
// Uso da capacidade (`progress` do Basecoat + marcador proprio)
// =============================================

/** Barra de capacidade com o marcador de aviso em 85% do `P_max`. */
export function capacityBar(total: number, pMax: number): string {
  const pct = Math.min(1, pMax > 0 ? total / pMax : 0);
  const over = pct >= 0.85;
  // A cor do trecho preenchido e ELEMENTO GRAFICO (3:1, FR-002f): usa a
  // variante base. O texto de aviso logo abaixo usa a `-ink`.
  const color = pct < 0.7 ? 'var(--green)' : pct < 0.85 ? 'var(--amber-line)' : 'var(--red)';
  const alert = over
    ? `<div class="alert mt-4" data-variant="destructive" role="status">
         ${icon('warn', 15)}
         <section><p>Perto do teto elétrico — novos comandos manuais podem ser recusados para manter a casa segura.</p></section>
       </div>`
    : '';
  return `
    <div>
      <div class="mb-3 flex items-baseline justify-between">
        <span class="text-sm font-bold text-ink">Uso da capacidade</span>
        <span class="tnum text-sm font-bold text-ink-soft">P_max: <b class="text-ink">${pMax.toLocaleString('pt-BR')} W</b></span>
      </div>
      <div class="relative">
        <div class="progress h-3" role="progressbar" aria-valuemin="0" aria-valuemax="${pMax}"
          aria-valuenow="${Math.round(total)}" aria-label="Uso da capacidade do circuito">
          <span style="width:${Math.max(2, pct * 100).toFixed(1)}%;background:${color}"></span>
        </div>
        <div class="cap-mark" style="left:85%"></div>
      </div>
      <div class="mt-2.5 flex justify-between text-xs font-semibold">
        <span class="tnum text-ink-faint">0 W</span>
        <span class="text-amber-ink">Limite de aviso (85%)</span>
        <span class="tnum text-red-ink">${pMax.toLocaleString('pt-BR')} W</span>
      </div>
      ${alert}
    </div>`;
}

// =============================================
// Teto do circuito (`range` do Basecoat)
// =============================================

/** Slider de potência (0..max). Input com data-role="power-slider". */
export function powerSlider(value: number, maxPadrao: number, label = 'Consumo máximo (Watts)'): string {
  // O teto do controle acompanha o valor gravado, arredondado para cima em
  // passos de 500 W. Com o maximo fixo em 3000 W, uma casa com P_max de 3500 W
  // — o do banco de demonstracao — via o polegar encalhado na ponta direita
  // mostrando um numero maior que o proprio fim da escala.
  const max = Math.max(maxPadrao, Math.ceil(value / 500) * 500);
  const pct = max > 0 ? (value / max) * 100 : 0;
  return `
    <div>
      <div class="mb-4 flex items-baseline justify-between gap-4">
        <label class="label text-sm font-bold" for="pmax">${label}</label>
        <span class="tnum rounded-lg bg-green-bg px-3 py-1.5 font-mono text-[13px] font-bold text-green-ink" data-role="power-val">${value.toLocaleString('pt-BR')} W</span>
      </div>
      <input id="pmax" class="input rng w-full" type="range" min="0" max="${max}" step="50" value="${value}"
        data-role="power-slider" style="--rng-pct:${pct.toFixed(1)}%"/>
      <div class="tnum mt-2.5 flex justify-between text-xs font-semibold text-ink-faint"><span>0 W</span><span>${max.toLocaleString('pt-BR')} W</span></div>
    </div>`;
}
