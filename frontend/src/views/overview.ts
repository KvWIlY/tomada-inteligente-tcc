// =============================================
// Visão Geral da Casa (porte do Overview do handoff)
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
  // FP baixo é a assinatura da fonte chaveada em standby — o alvo do trabalho.
  const fpTxt =
    fp === null
      ? ''
      : `<span style="color:${fp < 0.6 ? 'var(--amber, #b45309)' : 'var(--ink-faint)'}">FP ${fmt2(fp)}</span>`;
  return `
    <button class="tcard${on ? '' : ' tcard--off'}" data-id="${t.id}">
      <div class="tcard__top">
        <span class="tcard__icon${t.estado === 'ON' ? ' tcard__icon--on' : ''}">${applianceIcon(t, 19)}</span>
        <span class="tcard__name">${escapeHtml(t.nome)}</span>
        ${badge(t.estado)}
      </div>
      <div>
        <div class="eyebrow" style="margin-bottom:6px;font-weight:500">Consumo atual</div>
        <div class="tcard__bot">
          <div><span class="tcard__w-num tnum">${w}</span><span class="tcard__w-unit">W</span></div>
          ${miniBars(serie, t.estado)}
        </div>
        <div class="eyebrow" style="margin-top:8px;font-weight:500;display:flex;gap:10px;text-transform:none">
          <span><span class="tnum">${fmt2(kwh)}</span> kWh/24h</span>
          ${fpTxt}
        </div>
      </div>
    </button>`;
}

export function renderOverview(el: HTMLElement, p: OverviewProps): void {
  const pMax = p.residencia?.p_max ?? 2540;
  const connClass = p.online ? 'on' : 'offline';
  const connLabel = p.online ? 'Sistema online' : 'Offline · seguro';

  const eventos = Object.values(p.eventosPorTomada)
    .flat()
    .sort((a, b) => b.timestamp - a.timestamp)
    .slice(0, 8);

  // Energia da casa = soma por tomada, não integral do total instantâneo: as
  // séries têm buracos independentes (cada nó cai por conta própria), e integrar
  // a soma cobriria o buraco de uma com o dado da outra.
  const kwhCasa = p.tomadas.reduce((s, t) => s + energiaKWh(p.medicoesPorTomada[t.id] ?? []), 0);

  el.innerHTML = `
    <div class="content__inner">
      <div class="page-head">
        <div>
          <h2 class="page-title">Visão Geral da Casa</h2>
          <div class="page-sub">Saúde elétrica e consumo em tempo real · ${escapeHtml(p.residencia?.nome ?? 'Residência')}</div>
        </div>
        <div class="page-head__spacer"></div>
        <span class="online-pill online-pill--${connClass}"><span class="dot dot--ON"></span>${connLabel}</span>
      </div>

      <div class="ov-top">
        <div class="l-card">
          <div class="card__head" style="margin-bottom:0">
            <span class="eyebrow">Consumo total agora</span>
            <span style="color:var(--ink-faint)">${icon('gauge', 20)}</span>
          </div>
          <div class="bignum consumo-num tnum">${Math.round(p.total).toLocaleString('pt-BR')}<span class="bignum__unit">W</span></div>
          ${capacityBar(p.total, pMax)}
          <div class="card__sub" style="margin-top:12px">
            <strong class="tnum" style="color:var(--ink)">${fmt2(kwhCasa)}</strong> kWh nas últimas 24 h
            <span style="color:var(--ink-faint)"> · soma das ${p.tomadas.length} tomadas</span>
          </div>
        </div>

        <div class="l-card">
          <div class="card__head" style="margin-bottom:0">
            <span class="eyebrow">Pontuação de eficiência</span>
          </div>
          <div class="eff">
            ${efficiencyRing(p.score)}
            <div class="eff__cap">Quanto do consumo fantasma ocioso foi evitado abrindo o relé hoje.</div>
          </div>
        </div>
      </div>

      <div class="section">
        <div class="section__head">
          <h3 class="section__title">Tomadas Ativas</h3>
        </div>
        <div class="tgrid">
          ${p.tomadas.map((t) => tcard(t, p.medicoesPorTomada[t.id] ?? [])).join('')}
        </div>
      </div>

      <div class="section">
        <div class="section__head">
          <h3 class="section__title">Atividade do Otimizador</h3>
          <span class="card__sub" style="margin-top:0">por que o sistema decidiu o que decidiu</span>
        </div>
        <div class="l-card">
          ${eventsFeed(eventos)}
        </div>
      </div>
    </div>`;

  el.querySelectorAll<HTMLButtonElement>('.tcard[data-id]').forEach((btn) => {
    btn.addEventListener('click', () => {
      const id = btn.dataset.id;
      if (id) p.onSelect(id);
    });
  });
}
