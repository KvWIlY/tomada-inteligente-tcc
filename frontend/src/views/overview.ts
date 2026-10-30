// =============================================
// Visão Geral da Casa - cartoes do Basecoat
// =============================================

import { icon } from '../icons';
import { energiaKWh, fatorPotenciaAgregado, fmt2, seriesFromMedicoes } from '../derive';
import type { Evento, Medicao, Residencia, Tomada } from '../types';
import { applianceIcon, badge, escapeHtml, eventsFeed } from './components';
import { capacityBar, efficiencyRing, miniBars } from './charts';

export interface OverviewProps {
  residencia: Residencia | null;
  tomadas: Tomada[];
  medicoesPorTomada: Record<string, Medicao[]>;
  eventosPorTomada: Record<string, Evento[]>;
  total: number;
  score: number | null;
  online: boolean;
  onSelect: (id: string) => void;
}

function tcard(t: Tomada, medicoes: Medicao[]): string {
  const on = t.estado !== 'OFF';
  const serie = seriesFromMedicoes(medicoes, 24);
  const w = t.estado === 'OFF' ? '0' : Math.round(t.p_medida_atual).toLocaleString('pt-BR');
  const kwh = energiaKWh(medicoes);
  const fp = fatorPotenciaAgregado(medicoes);
  // FP baixo é a assinatura da fonte chaveada em standby - o alvo do trabalho.
  // O tom e a variante `-ink` (FR-002f): isto e TEXTO, e `--amber` rende 2,14:1
  // sobre o cartao claro, reprovando o 4,5:1 do FR-002c.
  const fpTxt =
    fp === null
      ? ''
      : `<span class="${fp < 0.6 ? 'text-amber-ink' : 'text-ink-faint'}">FP ${fmt2(fp)}</span>`;
  return `
    <button type="button" class="card gap-4 py-5 text-left transition hover:-translate-y-[3px] hover:shadow-lg${on ? '' : ' opacity-70'}" data-id="${t.id}">
      <header>
        <div class="flex items-center gap-3">
          <span class="grid size-9 shrink-0 place-items-center rounded-[11px] ${t.estado === 'ON' ? 'bg-green-bg text-green-deep' : 'bg-lav text-ink-soft'}">${applianceIcon(t, 19)}</span>
          <span class="min-w-0 flex-1 truncate text-[15px] leading-tight font-medium">${escapeHtml(t.nome)}</span>
          ${badge(t.estado)}
        </div>
      </header>
      <section>
        <div class="mb-1.5 text-[11.5px] font-extrabold tracking-[0.09em] text-ink uppercase">Consumo atual</div>
        <div class="flex items-end justify-between gap-3">
          <div><span class="tnum text-[26px] font-extrabold tracking-tighter">${w}</span><span class="ms-1 text-sm font-bold text-ink-faint">W</span></div>
          ${miniBars(serie, t.estado)}
        </div>
        <div class="mt-2 flex gap-2.5 text-[11.5px] font-medium text-ink">
          <span><span class="tnum">${fmt2(kwh)}</span> kWh/24h</span>
          ${fpTxt}
        </div>
      </section>
    </button>`;
}

export function renderOverview(el: HTMLElement, p: OverviewProps): void {
  const pMax = p.residencia?.p_max ?? 2540;

  const eventos = Object.values(p.eventosPorTomada)
    .flat()
    .sort((a, b) => b.timestamp - a.timestamp)
    .slice(0, 8);

  // Energia da casa = soma por tomada, não integral do total instantâneo: as
  // séries têm buracos independentes (cada nó cai por conta própria), e integrar
  // a soma cobriria o buraco de uma com o dado da outra.
  const kwhCasa = p.tomadas.reduce((s, t) => s + energiaKWh(p.medicoesPorTomada[t.id] ?? []), 0);

  el.innerHTML = `
    <div class="mx-auto max-w-[1080px] px-5 pt-6 pb-20 md:px-12 md:pt-9">
      <div class="mb-7 flex flex-col items-start gap-4 md:flex-row md:gap-5">
        <div>
          <h2 class="text-[27px] leading-tight font-bold tracking-tight md:text-[31px]">Visão Geral da Casa</h2>
          <p class="mt-1.5 text-[15px] font-medium text-ink-soft">Saúde elétrica e consumo em tempo real · ${escapeHtml(p.residencia?.nome ?? 'Residência')}</p>
        </div>
        <div class="flex-1"></div>
        <span class="badge gap-2 ${p.online ? 'badge--ON' : 'badge--OFF'}">
          <span class="dot ${p.online ? 'dot--ON dot--live' : 'dot--OFF'}"></span>${p.online ? 'Sistema online' : 'Offline · seguro'}
        </span>
      </div>

      <div class="grid gap-5 lg:grid-cols-[1.62fr_1fr]">
        <div class="card">
          <header class="flex flex-row items-center justify-between">
            <span class="text-[11.5px] font-extrabold tracking-[0.09em] text-ink uppercase">Consumo total agora</span>
            <span class="text-ink-faint">${icon('gauge', 20)}</span>
          </header>
          <section>
            <div class="tnum my-3 flex items-baseline gap-2 text-[clamp(44px,6vw,60px)] leading-[0.92] font-extrabold tracking-[-0.04em] text-green-num">
              ${Math.round(p.total).toLocaleString('pt-BR')}<span class="text-[0.32em] font-bold tracking-normal text-ink-faint">W</span>
            </div>
            ${capacityBar(p.total, pMax)}
            <p class="mt-3 text-[13px] font-medium text-ink-faint">
              <strong class="tnum text-ink">${fmt2(kwhCasa)}</strong> kWh nas últimas 24 h · soma das ${p.tomadas.length} tomadas
            </p>
          </section>
        </div>

        <div class="card">
          <header>
            <span class="text-[11.5px] font-extrabold tracking-[0.09em] text-ink uppercase">Pontuação de eficiência</span>
          </header>
          <section class="flex flex-col items-center">
            ${efficiencyRing(p.score)}
            <p class="mt-1.5 max-w-[30ch] text-center text-[13px] font-medium text-ink-soft">Quanto do consumo fantasma ocioso foi evitado abrindo o relé hoje.</p>
          </section>
        </div>
      </div>

      <section class="mt-8">
        <h3 class="mb-4 text-[19.5px] font-bold tracking-tight">Tomadas Ativas</h3>
        <div class="grid gap-4 [grid-template-columns:repeat(auto-fill,minmax(min(100%,290px),1fr))]">
          ${p.tomadas.map((t) => tcard(t, p.medicoesPorTomada[t.id] ?? [])).join('')}
        </div>
      </section>

      <section class="mt-8">
        <h3 class="mb-4 text-[19.5px] font-bold tracking-tight">Atividade do Otimizador</h3>
        <div class="card">
          <section>${eventsFeed(eventos)}</section>
        </div>
      </section>
    </div>`;

  el.querySelectorAll<HTMLButtonElement>('button[data-id]').forEach((btn) => {
    btn.addEventListener('click', () => {
      const id = btn.dataset.id;
      if (id) p.onSelect(id);
    });
  });
}
