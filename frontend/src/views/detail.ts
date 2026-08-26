// =============================================
// Detalhe da Tomada — cartoes e abas do Basecoat
// =============================================
//
// Saiu daqui na feature 007 (FR-009, FR-009a):
//   * o cartao "Agendamento Operacional" com as 24 horas pintaveis — o
//     agendamento deixou de fazer parte do produto;
//   * o cartao "Probabilidade de Uso" — exibia `pi_historico`, que NADA no
//     sistema calcula (o firmware nunca o preenche);
//   * a funcao `build7d`, que colapsava 7 dias reais numa media por hora e
//     multiplicava por um fator de variacao inventado. O seletor "7 dias"
//     agora plota a serie que o backend devolve (FR-018).
//
// Os campos `schedule` e `pi_historico` continuam INTACTOS no contrato e no
// banco (FR-009b, FR-010a): saiu a tela, nao o dado.

import { icon } from '../icons';
import { energiaKWh, fatorPotenciaAgregado, fmt2, seriesFromMedicoes } from '../derive';
import type { Medicao, Residencia, Tomada } from '../types';
import { badge, critBadge, escapeHtml, stateDot, toggle } from './components';
import { carimbosDaSerie, consumptionChart, montarConsumo, powerSlider } from './charts';

/** Pontos plotados por janela. 24 h e a cadencia de ~60 s reamostrada; 7 dias
 *  reduz 7×24 h a um ponto por hora, sem inventar nada entre eles. */
const PONTOS: Record<'24h' | '7d', number> = { '24h': 60, '7d': 168 };

export interface DetailProps {
  tomada: Tomada;
  residencia: Residencia | null;
  /** Sempre as 24 h reais — base de kWh e fator de potência. */
  medicoes: Medicao[];
  /** A série da janela escolhida (igual a `medicoes` quando o seletor está em 24 h). */
  medicoesJanela: Medicao[];
  online: boolean;
  seg: '24h' | '7d';
  draftCritical: boolean;
  dirty: boolean;
  saving: boolean;
  /** As tres intencoes do dono sobre o rele (ver `ComandoRele` em services/api). */
  onComandoRele: (cmd: 'auto' | 'usar_agora' | 'cortar') => void;
  onChangeSeg: (s: '24h' | '7d') => void;
  onToggleCritical: () => void;
  onSaveConfig: () => void;
  onSavePMax: (w: number) => void;
  /** UC26: desassocia o hardware e rotaciona a chave da casa (feature 005). */
  onRevogar: () => void;
}

/** Rótulo do modo vigente. "Usar agora" é temporário: o ESP32 devolve o controle
 *  ao otimizador ao fim da janela e o campo volta a `null` sozinho. */
function modoRotulo(rele: boolean | null): string {
  if (rele === null) return 'Automático · o otimizador decide';
  return rele ? 'Cortada manualmente' : 'Ligada por você · volta ao automático em até 30 min';
}

/** Cor do fator de potência. Abaixo de 0,6 é o território das fontes chaveadas
 *  em standby — a carga que o trabalho existe para cortar. `null` = medição
 *  antiga, sem fase gravada: cinza, nunca vermelho (ausência não é problema).
 *  Sempre a variante `-ink`: isto é TEXTO (FR-002f). */
function fpClasse(fp: number | null): string {
  if (fp === null) return 'text-ink-faint';
  return fp < 0.6 ? 'text-amber-ink' : 'text-green-ink';
}

function modoClasse(rele: boolean | null): string {
  if (rele === null) return 'text-ink-faint';
  return rele ? 'text-ink-faint' : 'text-green-ink';
}

/** Uma aba do `tabs` do Basecoat (usado no tri-estado e no seletor de janela). */
function aba(rotulo: string, ativo: boolean, attrs: string, titulo = ''): string {
  return `<button type="button" role="tab" aria-selected="${ativo}" ${attrs}
    ${titulo ? `title="${escapeHtml(titulo)}"` : ''}>${rotulo}</button>`;
}

export function renderDetail(el: HTMLElement, p: DetailProps): void {
  const t = p.tomada;
  const pMax = p.residencia?.p_max ?? 2540;

  // Estado vazio: tomada pareada, ainda sem medições.
  if (p.medicoes.length === 0) {
    el.innerHTML = `
      <div class="mx-auto max-w-[1080px] px-5 pt-6 pb-20 md:px-12 md:pt-9">
        <div class="mb-7">
          <h2 class="text-[27px] font-bold tracking-tight md:text-[31px]">${escapeHtml(t.nome)}</h2>
          <p class="mt-1.5 text-[15px] font-medium text-ink-soft">${escapeHtml(t.localizacao ?? 'Dispositivo')}</p>
        </div>
        <div class="card">
          <section class="empty py-12">
            <header>
              <figure class="mb-4 grid size-14 place-items-center rounded-2xl bg-lav text-ink-faint">${icon('plug', 26)}</figure>
              <h3 class="text-xl font-bold tracking-tight">Tomada pareada, sem dados ainda</h3>
              <p class="mt-2 text-sm text-ink-soft">Assim que o aparelho for usado, as medições começam a aparecer aqui —
                 potência ativa, energia acumulada e fator de potência.</p>
            </header>
          </section>
        </div>
      </div>`;
    return;
  }

  const n = PONTOS[p.seg];
  const serie = seriesFromMedicoes(p.medicoesJanela, n);
  const carimbos = carimbosDaSerie(p.medicoesJanela, n);
  const media = Math.round(serie.reduce((s, v) => s + v, 0) / serie.length);
  const pico = Math.round(Math.max(...serie));
  // Energia e FP saem SEMPRE das medições de 24 h, mesmo com o seletor em
  // "7 dias": são os números que sustentam a tese do trabalho e a janela curta
  // é a que o dono está olhando no cabeçalho.
  const kwh = energiaKWh(p.medicoes);
  const fp = fatorPotenciaAgregado(p.medicoes);
  const syncLabel = p.online ? 'Sincronização ESP32 ao vivo' : 'Offline — decidindo localmente';
  const pNum = t.estado === 'OFF' ? '0' : Math.round(t.p_medida_atual).toLocaleString('pt-BR');
  const identity = t.mac ?? t.id;

  el.innerHTML = `
    <div class="mx-auto max-w-[1080px] px-5 pt-6 pb-20 md:px-12 md:pt-9">
      <div class="flex flex-col gap-5">

        <!-- hero -->
        <div class="card">
          <section class="grid items-stretch gap-6 md:grid-cols-[1fr_auto]">
            <div>
              <div class="mb-1.5 flex flex-wrap items-center gap-3">
                <h2 class="text-[29px] font-bold tracking-tight">${escapeHtml(t.nome)}</h2>
                ${badge(t.estado)}
                ${p.draftCritical ? critBadge() : ''}
              </div>
              <div class="tnum my-2 flex items-baseline gap-2 text-[clamp(40px,5.5vw,54px)] leading-[0.92] font-extrabold tracking-[-0.04em] text-green-num">
                ${pNum}<span class="text-[0.32em] font-bold tracking-normal text-ink-faint">W</span>
              </div>
              <span class="inline-flex items-center gap-2 text-[13.5px] font-semibold text-ink-soft">${stateDot(p.online ? 'ON' : 'STD', p.online)}${syncLabel}</span>
            </div>
            <div class="flex flex-col justify-center gap-2.5 rounded-2xl bg-surface p-4 md:min-w-[260px]">
              <span class="text-[11px] font-extrabold tracking-[0.1em] text-ink-soft uppercase">Controle Manual</span>
              <div class="tabs">
                <div role="tablist" aria-label="Controle manual do relé" class="w-full">
                  ${aba('Automático', t.rele_aberto === null, 'data-cmd="auto"', 'O otimizador decide')}
                  ${aba('Usar agora', t.rele_aberto === false, 'data-cmd="usar_agora"', 'Religa por 30 min e volta sozinha ao automático')}
                  ${aba('Cortar', t.rele_aberto === true, 'data-cmd="cortar"', 'Mantém desligada até você desfazer')}
                </div>
              </div>
              <span class="text-[13px] font-bold ${modoClasse(t.rele_aberto)}">${modoRotulo(t.rele_aberto)}</span>
            </div>
          </section>
        </div>

        <!-- histórico -->
        <div class="card">
          <header class="flex flex-row flex-wrap items-start justify-between gap-4">
            <div>
              <h3>Histórico de Consumo</h3>
              <p data-description>${p.seg === '24h' ? 'Últimas 24 horas' : 'Últimos 7 dias'} · média ${media} W · pico ${pico} W</p>
              <p class="mt-1.5 flex flex-wrap gap-4 text-[13px] font-medium text-ink-faint">
                <span title="Energia integrada sobre os timestamps reais das últimas 24 h"><strong class="tnum text-ink">${fmt2(kwh)}</strong> kWh em 24 h</span>
                <span title="${fp === null ? 'Medições anteriores a 2026-08-24 não gravaram fase' : 'Razão entre potência ativa e aparente (ΣP/ΣS)'}">fator de potência <strong class="tnum ${fpClasse(fp)}">${fp === null ? '—' : fmt2(fp)}</strong></span>
              </p>
            </div>
            <div class="tabs">
              <div role="tablist" aria-label="Janela do histórico">
                ${aba('24h', p.seg === '24h', 'data-seg="24h"')}
                ${aba('7 dias', p.seg === '7d', 'data-seg="7d"')}
              </div>
            </div>
          </header>
          <section>${consumptionChart(serie, carimbos)}</section>
        </div>

        <!-- limites de potência -->
        <div class="card">
          <header class="flex flex-row items-center">
            <h3 class="flex items-center gap-2.5"><span class="text-green">${icon('gauge', 20)}</span>Limites de Potência</h3>
          </header>
          <section>
            ${powerSlider(pMax, 3000, 'Teto elétrico do circuito · P_max (Watts)')}

            <div class="mt-5 flex items-start gap-3 rounded-xl bg-lav p-4 text-[13.5px] font-medium text-ink-soft">
              <span class="mt-px shrink-0 text-green">${icon('info', 17)}</span>
              O P_max é o teto do circuito inteiro. Se o consumo somado das tomadas se aproximar dele, novos comandos manuais podem ser recusados.
            </div>

            <div class="mt-2">
              <div class="field flex-row items-center justify-between gap-4 border-b border-line-soft py-4" data-orientation="horizontal">
                <section class="min-w-0 flex-1">
                  <label class="label text-[14.5px] font-bold" for="critica">Tomada crítica</label>
                  <p class="mt-0.5 max-w-[64ch] text-[12.5px] font-medium text-ink-faint">Aparelhos essenciais (geladeira, roteador) nunca são desligados pelo otimizador.</p>
                </section>
                ${toggle(p.draftCritical, 'id="critica" data-role="critical"', 'Marcar como tomada crítica')}
              </div>

              <div class="flex items-center justify-between gap-4 border-b border-line-soft py-4">
                <div class="min-w-0 flex-1">
                  <div class="text-[14.5px] font-bold">Identidade do dispositivo</div>
                  <p class="mt-0.5 max-w-[64ch] text-[12.5px] font-medium text-ink-faint">Identificador do ESP32. Somente leitura.</p>
                </div>
                <code class="rounded-lg border border-border bg-surface px-2.5 py-1.5 font-mono text-[13px] text-ink-soft">${escapeHtml(identity)}</code>
              </div>

              <div class="flex flex-wrap items-center justify-between gap-4 py-4">
                <div class="min-w-0 flex-1">
                  <div class="text-[14.5px] font-bold">Revogar dispositivo</div>
                  <p class="mt-0.5 max-w-[64ch] text-[12.5px] font-medium text-ink-faint">Desassocia este ESP32 em caso de venda, troca ou perda.
                    O histórico é preservado e a chave da casa é trocada, e as outras tomadas recebem a nova sozinhas.</p>
                </div>
                <button type="button" class="btn" data-variant="destructive" data-role="revogar">Revogar</button>
              </div>
            </div>
          </section>
          <footer class="flex-wrap justify-between gap-3">
            <span class="text-[12.5px] font-semibold text-ink-faint">Configuração · versão atual
              <code class="rounded-md border border-border bg-surface px-2 py-0.5 font-mono">v${t.config_version}</code></span>
            <button type="button" class="btn" data-role="save" ${!p.dirty || p.saving ? 'disabled' : ''}>${p.saving ? 'Salvando…' : p.dirty ? `Salvar (v${t.config_version + 1})` : 'Tudo salvo'}</button>
          </footer>
        </div>
      </div>
    </div>`;

  montarConsumo(el, serie, carimbos, t.estado);

  // --- listeners ---
  el.querySelectorAll<HTMLButtonElement>('[data-cmd]').forEach((btn) =>
    btn.addEventListener('click', () =>
      p.onComandoRele(btn.dataset.cmd as 'auto' | 'usar_agora' | 'cortar'),
    ),
  );
  el.querySelector('[data-role="critical"]')?.addEventListener('change', () => p.onToggleCritical());
  el.querySelector('[data-role="save"]')?.addEventListener('click', () => p.onSaveConfig());
  el.querySelector('[data-role="revogar"]')?.addEventListener('click', () => p.onRevogar());

  el.querySelectorAll<HTMLButtonElement>('[data-seg]').forEach((btn) =>
    btn.addEventListener('click', () => p.onChangeSeg(btn.dataset.seg as '24h' | '7d')),
  );

  // Slider de P_max: atualiza rótulo e trilha ao vivo; persiste só no 'change'
  // (FR-005e item 3) — persistir a cada 'input' mandaria um PATCH por pixel.
  const slider = el.querySelector<HTMLInputElement>('[data-role="power-slider"]');
  const valEl = el.querySelector<HTMLElement>('[data-role="power-val"]');
  if (slider) {
    slider.addEventListener('input', () => {
      const v = Number(slider.value);
      slider.style.setProperty('--rng-pct', `${((v / Number(slider.max)) * 100).toFixed(1)}%`);
      if (valEl) valEl.textContent = `${v.toLocaleString('pt-BR')} W`;
    });
    slider.addEventListener('change', () => p.onSavePMax(Number(slider.value)));
  }
}
