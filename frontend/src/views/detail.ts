// =============================================
// Detalhe da Tomada (porte do Detail do handoff + agendamento real)
// =============================================

import { icon } from '../icons';
import { seriesFromMedicoes } from '../derive';
import type { Medicao, Residencia, Tomada } from '../types';
import { badge, critBadge, escapeHtml, stateDot, toggle } from './components';
import { consumptionChart, powerSlider, probabilityChart, scheduleBar } from './charts';

export interface DetailProps {
  tomada: Tomada;
  residencia: Residencia | null;
  medicoes: Medicao[];
  online: boolean;
  seg: '24h' | '7d';
  draftSchedule: boolean[];
  draftCritical: boolean;
  dirty: boolean;
  saving: boolean;
  onToggleRele: () => void;
  onChangeSeg: (s: '24h' | '7d') => void;
  onScheduleToggle: (h: number) => void;
  onToggleCritical: () => void;
  onSaveConfig: () => void;
  onSavePMax: (w: number) => void;
  /** UC26: desassocia o hardware e rotaciona a chave da casa (feature 005). */
  onRevogar: () => void;
}

/** Série de 7 dias estimada a partir da média por hora das 24h reais. */
function build7d(medicoes: Medicao[]): number[] {
  const out: number[] = [];
  for (let d = 6; d >= 0; d--) {
    for (let h = 0; h < 24; h++) {
      const sample = medicoes.filter((m) => new Date(m.timestamp * 1000).getHours() === h);
      const avg = sample.length ? sample.reduce((s, m) => s + m.p_medida, 0) / sample.length : 0;
      const scale = 0.88 + ((d * 3 + h) % 7) * 0.04;
      out.push(avg * scale);
    }
  }
  return out;
}

export function renderDetail(el: HTMLElement, p: DetailProps): void {
  const t = p.tomada;
  const pMax = p.residencia?.p_max ?? 2540;
  const hourNow = new Date().getHours();

  // Estado vazio: tomada pareada, ainda sem medições.
  if (p.medicoes.length === 0) {
    el.innerHTML = `
      <div class="content__inner">
        <div class="page-head"><div><h2 class="page-title">${escapeHtml(t.nome)}</h2><div class="page-sub">${escapeHtml(t.localizacao ?? 'Dispositivo')}</div></div></div>
        <div class="card">
          <div class="empty">
            <div class="empty__mark">${icon('plus', 26, 2.2)}</div>
            <div class="empty__title">Tomada pareada, sem dados ainda</div>
            <div class="empty__sub">Assim que o aparelho for usado, as medições começam a aparecer aqui — consumo, probabilidade de uso e histórico.</div>
          </div>
        </div>
      </div>`;
    return;
  }

  const serie = p.seg === '24h' ? seriesFromMedicoes(p.medicoes, 60) : build7d(p.medicoes);
  const media = Math.round(serie.reduce((s, v) => s + v, 0) / serie.length);
  const pico = Math.round(Math.max(...serie));
  const syncLabel = p.online ? 'Sincronização ESP32 ao vivo' : 'Offline — decidindo localmente';
  const pNum = t.estado === 'OFF' ? '0' : Math.round(t.p_medida_atual).toLocaleString('pt-BR');
  const identity = t.mac ?? t.id;

  el.innerHTML = `
    <div class="content__inner">
      <div class="detail-stack">
        <!-- hero -->
        <div class="card" style="padding:0;overflow:hidden">
          <div class="dhero" style="padding:28px">
            <div>
              <div class="dhero__titrow">
                <h2 class="dhero__title">${escapeHtml(t.nome)}</h2>
                ${badge(t.estado)}
                ${p.draftCritical ? critBadge() : ''}
              </div>
              <div class="bignum dhero__num tnum">${pNum}<span class="bignum__unit">W</span></div>
              <span class="dhero__sync">${stateDot(p.online ? 'ON' : 'STD', p.online)}${syncLabel}</span>
            </div>
            <div class="manual">
              <span class="manual__label">Controle Manual</span>
              ${toggle(!t.rele_aberto, 'data-role="rele"')}
              <span class="manual__state" style="color:${t.rele_aberto ? 'var(--ink-faint)' : 'var(--green-deep)'}">${t.rele_aberto ? 'Desligada' : 'Ligada'}</span>
            </div>
          </div>
        </div>

        <!-- gráficos -->
        <div class="grid-2">
          <div class="card" style="display:flex;flex-direction:column">
            <div class="card__head">
              <div>
                <h3 class="card__title">Histórico de Consumo</h3>
                <div class="card__sub">${p.seg === '24h' ? 'Últimas 24 horas' : 'Últimos 7 dias (est.)'} · média ${media} W · pico ${pico} W</div>
              </div>
              <div class="seg">
                <button class="${p.seg === '24h' ? 'is-active' : ''}" data-seg="24h">24h</button>
                <button class="${p.seg === '7d' ? 'is-active' : ''}" data-seg="7d">7 dias</button>
              </div>
            </div>
            <div style="flex:1;display:grid;place-items:center">
              ${consumptionChart(serie, t.estado)}
            </div>
          </div>

          <div class="card">
            <div class="card__head">
              <div>
                <h3 class="card__title">Probabilidade de Uso</h3>
                <div class="card__sub">Atividade esperada por hora</div>
              </div>
            </div>
            ${probabilityChart(t.config.pi_historico, hourNow)}
            <div class="infobox" style="margin-top:14px">${icon('info', 17)}O otimizador evita desligar este aparelho nas horas mais prováveis (barra clara = agora).</div>
          </div>
        </div>

        <!-- agendamento operacional -->
        <div class="card">
          <div class="card__head" style="align-items:center">
            <div>
              <h3 class="card__title" style="display:flex;align-items:center;gap:10px"><span style="color:var(--green)">${icon('grid', 20)}</span>Agendamento Operacional</h3>
              <div class="card__sub">Pinte as horas permitidas (verde) e bloqueadas (cinza).</div>
            </div>
          </div>
          ${scheduleBar(p.draftSchedule, p.saving)}
          <div class="sched__note">Fora das horas permitidas, o otimizador prioriza manter este aparelho desligado (restrição R5 do modelo).</div>
        </div>

        <!-- limites de potência -->
        <div class="card">
          <div class="card__head" style="align-items:center">
            <h3 class="card__title" style="display:flex;align-items:center;gap:10px"><span style="color:var(--green)">${icon('gauge', 20)}</span>Limites de Potência</h3>
          </div>
          ${powerSlider(pMax, 3000, 'Teto elétrico do circuito · P_max (Watts)')}
          <div class="infobox">${icon('info', 17)}O P_max é o teto do circuito inteiro. Se o consumo somado das tomadas se aproximar dele, novos comandos manuais podem ser recusados.</div>

          <div style="margin-top:8px">
            <div class="crow">
              <div>
                <div class="crow__label">Tomada crítica</div>
                <div class="crow__desc">Aparelhos essenciais (geladeira, roteador) nunca são desligados pelo otimizador.</div>
              </div>
              ${toggle(p.draftCritical, 'data-role="critical"')}
            </div>
            <div class="crow">
              <div>
                <div class="crow__label">Identidade do dispositivo</div>
                <div class="crow__desc">Identificador do ESP32 — somente leitura.</div>
              </div>
              <code>${escapeHtml(identity)}</code>
            </div>
            <div class="crow">
              <div>
                <div class="crow__label">Revogar dispositivo</div>
                <div class="crow__desc">Desassocia este ESP32 (venda, troca ou perda). O histórico é preservado e a chave de coordenação da casa é trocada — as outras tomadas recebem a nova sozinhas. Para religar, é preciso reprovisionar com um novo código.</div>
              </div>
              <button class="btn btn--danger" data-role="revogar">Revogar</button>
            </div>
          </div>

          <div class="savebar">
            <span class="savebar__ver">Configuração · versão atual <code>v${t.config_version}</code></span>
            <button class="btn btn--green" data-role="save" ${!p.dirty || p.saving ? 'disabled' : ''}>${p.saving ? 'Salvando…' : p.dirty ? `Salvar (v${t.config_version + 1})` : 'Tudo salvo'}</button>
          </div>
        </div>
      </div>
    </div>`;

  // --- listeners ---
  el.querySelector('[data-role="rele"]')?.addEventListener('click', () => p.onToggleRele());
  el.querySelector('[data-role="critical"]')?.addEventListener('click', () => p.onToggleCritical());
  el.querySelector('[data-role="save"]')?.addEventListener('click', () => p.onSaveConfig());

  // Revogação em 2 passos: é irreversível sem reprovisionar o hardware, então o
  // usuário digita o nome da tomada para confirmar (evita clique acidental).
  el.querySelector('[data-role="revogar"]')?.addEventListener('click', () => {
    const digitado = window.prompt(
      `Revogar "${t.nome}"?\n\nO histórico é mantido, mas o ESP32 perde o acesso e a chave da casa é trocada.\nDigite o nome da tomada para confirmar:`,
    );
    if (digitado !== null && digitado.trim() === t.nome.trim()) p.onRevogar();
  });

  el.querySelectorAll<HTMLButtonElement>('[data-seg]').forEach((btn) =>
    btn.addEventListener('click', () => p.onChangeSeg(btn.dataset.seg as '24h' | '7d')),
  );

  el.querySelectorAll<HTMLButtonElement>('.sched__seg[data-h]').forEach((btn) =>
    btn.addEventListener('click', () => {
      const h = Number(btn.dataset.h);
      if (!Number.isNaN(h)) p.onScheduleToggle(h);
    }),
  );

  // Slider de P_max: atualiza label/gradiente ao vivo; persiste no 'change'.
  const slider = el.querySelector<HTMLInputElement>('[data-role="power-slider"]');
  const valEl = el.querySelector<HTMLElement>('[data-role="power-val"]');
  if (slider) {
    slider.addEventListener('input', () => {
      const v = Number(slider.value);
      const pct = (v / Number(slider.max)) * 100;
      slider.style.background = `linear-gradient(to right, var(--green) 0%, var(--green) ${pct}%, var(--lav-track) ${pct}%, var(--lav-track) 100%)`;
      if (valEl) valEl.textContent = `${v.toLocaleString('pt-BR')} W`;
    });
    slider.addEventListener('change', () => p.onSavePMax(Number(slider.value)));
  }
}
