// =============================================
// Bootstrap do dashboard -- conecta tudo
// =============================================

import { buildMockData } from './mock';
import { createStore, loadTweaks, saveTweaks } from './store';
import * as supa from './supabase';
import type { AppState } from './types';
import { aplicarTweaks, calcEstadoAtual } from './utils';
import { renderChart, type ChartWindow } from './views/chart';
import { renderConfig } from './views/config';
import { renderEvents, type EventFilter } from './views/events';
import { renderHero } from './views/hero';
import { renderMetrics } from './views/metrics';
import { renderOverview } from './views/overview';
import { renderPicker } from './views/picker';
import { renderToast } from './views/toast';
import { renderTopbar } from './views/topbar';
import { renderTweaks } from './views/tweaks';

const RESIDENCIA_ID = import.meta.env.VITE_RESIDENCIA_ID as string | undefined;

// === 1. Tweaks persistidos ===
const tweaks = loadTweaks();

// === 2. Store inicial (tudo vazio enquanto fetch nao volta) ===
const store = createStore<AppState>({
  residencia: null,
  tomadas: [],
  medicoesPorTomada: {},
  eventosPorTomada: {},
  selectedId: null,
  showAll: true,
  scheduleStyle: (tweaks.scheduleStyle as AppState['scheduleStyle']) ?? 'grid',
  theme:         (tweaks.theme         as AppState['theme'])         ?? 'light',
  accent:        (tweaks.accent        as AppState['accent'])        ?? 'violet',
  density:       (tweaks.density       as AppState['density'])       ?? 'comfortable',
  estadoOverride:(tweaks.estadoOverride as AppState['estadoOverride'])?? 'auto',
  loading: true,
  error: null,
  toast: null,
});

// === 3. Aplica tweaks visuais ===
{
  const s = store.get();
  aplicarTweaks(s.theme, s.accent, s.density);
}

// === 4. Estado de UI local (chart window, event filter) ===
let chartWindow: ChartWindow = '24h';
let eventFilter: EventFilter = 'all';
let savingConfig = false;

// Helper de toast: seta a msg e agenda LIMPEZA do store apos 2.8s.
// Sem isso, qualquer re-render (ex.: nova medicao via Realtime) re-mostra o toast.
let toastTimer: number | null = null;
function showToast(msg: string): void {
  store.set({ toast: msg });
  if (toastTimer !== null) clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => {
    store.set({ toast: null });
    toastTimer = null;
  }, 2800);
}

// === 5. Carrega dados iniciais ===
async function bootstrap() {
  if (!supa.hasSupabase || !RESIDENCIA_ID) {
    console.warn('[dashboard] Supabase nao configurado -- usando dados MOCK');
    const mock = buildMockData();
    store.set({
      residencia: mock.residencia,
      tomadas: mock.tomadas,
      medicoesPorTomada: mock.medicoesPorTomada,
      eventosPorTomada: mock.eventosPorTomada,
      selectedId: mock.tomadas[0]?.id ?? null,
      loading: false,
    });
    return;
  }

  try {
    const [residencia, tomadas] = await Promise.all([
      supa.fetchResidencia(RESIDENCIA_ID),
      supa.fetchTomadas(RESIDENCIA_ID),
    ]);

    if (!residencia) {
      store.set({ loading: false, error: 'Residencia nao encontrada' });
      return;
    }

    // Carrega medicoes + eventos de cada tomada em paralelo
    const medicoesPorTomada: Record<string, Awaited<ReturnType<typeof supa.fetchMedicoes>>> = {};
    const eventosPorTomada: Record<string, Awaited<ReturnType<typeof supa.fetchEventos>>> = {};
    await Promise.all(
      tomadas.map(async (t) => {
        const [meds, evs] = await Promise.all([
          supa.fetchMedicoes(t.id, 24),
          supa.fetchEventos(t.id, 100),
        ]);
        medicoesPorTomada[t.id] = meds;
        eventosPorTomada[t.id] = evs;
      }),
    );

    store.set({
      residencia,
      tomadas,
      medicoesPorTomada,
      eventosPorTomada,
      selectedId: tomadas[0]?.id ?? null,
      loading: false,
    });

    // === 6. Realtime: nova medicao chega, atualiza store ===
    for (const t of tomadas) {
      supa.subscribeMedicoes(t.id, (m) => {
        const cur = store.get();
        const arr = cur.medicoesPorTomada[t.id] ?? [];
        store.set({
          medicoesPorTomada: { ...cur.medicoesPorTomada, [t.id]: [...arr, m].slice(-2000) },
        });
      });
      supa.subscribeEventos(t.id, (e) => {
        const cur = store.get();
        const arr = cur.eventosPorTomada[t.id] ?? [];
        store.set({
          eventosPorTomada: { ...cur.eventosPorTomada, [t.id]: [e, ...arr].slice(0, 200) },
        });
      });
    }
    supa.subscribeTomadas(RESIDENCIA_ID, (t) => {
      const cur = store.get();
      const tomadasNovas = cur.tomadas.map((tt) => (tt.id === t.id ? t : tt));
      store.set({ tomadas: tomadasNovas });
    });
  } catch (err) {
    console.error('[dashboard] bootstrap falhou:', err);
    store.set({
      loading: false,
      error: err instanceof Error ? err.message : 'Erro desconhecido',
    });
  }
}

// === 7. Subscriptions: cada view re-renderiza quando estado muda ===
const elPicker  = document.getElementById('picker')!;
const elTopbar  = document.getElementById('topbar')!;
const elContent = document.getElementById('content')!;
const elTweaks  = document.getElementById('tweaks-panel')!;
const elToast   = document.getElementById('toast')!;

store.subscribe((s) => {
  // Aplica tweaks visuais sempre que mudarem
  aplicarTweaks(s.theme, s.accent, s.density);

  // Picker (sidebar)
  renderPicker(elPicker, {
    tomadas: s.tomadas,
    medicoesPorTomada: s.medicoesPorTomada,
    selectedId: s.selectedId,
    showAll: s.showAll,
    onSelect: (id) => store.set({ selectedId: id, showAll: false }),
    onShowAll: () => store.set({ showAll: true }),
  });

  // Topbar
  renderTopbar(elTopbar, {
    residencia: s.residencia,
    tomadas: s.tomadas,
    medicoesPorTomada: s.medicoesPorTomada,
    online: !s.error,
  });

  // Tweaks panel
  renderTweaks(elTweaks, {
    theme: s.theme,
    accent: s.accent,
    density: s.density,
    scheduleStyle: s.scheduleStyle,
    estadoOverride: s.estadoOverride,
    onChangeTheme: (t) => store.set({ theme: t }),
    onChangeAccent: (a) => store.set({ accent: a }),
    onChangeDensity: (d) => store.set({ density: d }),
    onChangeScheduleStyle: (st) => store.set({ scheduleStyle: st }),
    onChangeEstadoOverride: (e) => store.set({ estadoOverride: e }),
  });

  // Toast
  renderToast(elToast, s.toast);

  // Content
  if (s.loading) {
    elContent.innerHTML = `<div class="placeholder"><p>Carregando dados do Supabase...</p></div>`;
    return;
  }
  if (s.error) {
    elContent.innerHTML = `
      <div class="placeholder">
        <h2 style="color:var(--off)">Erro: ${escapeHtml(s.error)}</h2>
        <p>Verifique <code>.env</code> e o painel do Supabase. Caindo em modo offline.</p>
      </div>
    `;
    return;
  }
  if (s.tomadas.length === 0) {
    elContent.innerHTML = `
      <div class="placeholder">
        <h2>Nenhuma tomada cadastrada</h2>
        <p>Crie uma tomada na residencia ${escapeHtml(s.residencia?.nome ?? '')} no Supabase.</p>
      </div>
    `;
    return;
  }

  // === Visao geral ===
  if (s.showAll) {
    elContent.innerHTML = '';
    renderOverview(elContent, {
      residencia: s.residencia,
      tomadas: s.tomadas,
      medicoesPorTomada: s.medicoesPorTomada,
      onOpenTomada: (id) => store.set({ selectedId: id, showAll: false }),
    });
    return;
  }

  // === Detalhe de uma tomada ===
  const tomada = s.tomadas.find((t) => t.id === s.selectedId) ?? s.tomadas[0];
  if (!tomada) return;
  const medicoes = s.medicoesPorTomada[tomada.id] ?? [];
  const eventos = s.eventosPorTomada[tomada.id] ?? [];
  const estadoAtual = calcEstadoAtual(
    tomada,
    medicoes,
    s.estadoOverride === 'auto' ? null : s.estadoOverride,
  );

  elContent.innerHTML = `
    <div style="display:flex;flex-direction:column;gap:6px">
      <div id="hero-slot"></div>
      <div id="metrics-slot"></div>
      <div id="chart-slot"></div>
      <div class="grid-2col">
        <div id="events-slot"></div>
        <div id="config-slot"></div>
      </div>
    </div>
  `;

  renderHero(document.getElementById('hero-slot')!, {
    tomada,
    residencia: s.residencia,
    estadoAtual,
    onToggleRele: async () => {
      const novoAberto = !tomada.rele_aberto;
      // Otimista
      const tomadasNovas = s.tomadas.map((t) =>
        t.id === tomada.id ? { ...t, rele_aberto: novoAberto } : t,
      );
      store.set({ tomadas: tomadasNovas });
      showToast('Comando enviado · aguardando ESP32');

      if (supa.hasSupabase) {
        try {
          await supa.setReleAberto(tomada.id, novoAberto);
        } catch (err) {
          console.error('[main] setReleAberto:', err);
          // Reverte em caso de erro
          store.set({ tomadas: s.tomadas });
          showToast('Falha ao enviar comando');
        }
      }
    },
  });

  renderMetrics(document.getElementById('metrics-slot')!, {
    medicoes,
    cstd: tomada.cstd,
    chaveamentos: eventos.filter((e) => e.tipo === 'chaveamento').length,
  });

  renderChart(document.getElementById('chart-slot')!, {
    medicoes,
    pLimiar: tomada.config.p_limiar_detectado_auto ?? 5,
    win: chartWindow,
    onChangeWin: (w) => {
      chartWindow = w;
      // Re-render so do chart
      renderChart(document.getElementById('chart-slot')!, {
        medicoes,
        pLimiar: tomada.config.p_limiar_detectado_auto ?? 5,
        win: chartWindow,
        onChangeWin: (w2) => {
          chartWindow = w2;
          store.set({}); // forca re-render geral
        },
      });
    },
  });

  renderEvents(document.getElementById('events-slot')!, {
    eventos,
    filter: eventFilter,
    onChangeFilter: (f) => {
      eventFilter = f;
      store.set({}); // re-render
    },
  });

  renderConfig(document.getElementById('config-slot')!, {
    tomada,
    scheduleStyle: s.scheduleStyle,
    onChangeScheduleStyle: (style) => store.set({ scheduleStyle: style }),
    saving: savingConfig,
    onSave: async (newConfig) => {
      savingConfig = true;
      showToast('Salvando configuracao...');
      const novaVersao = tomada.config_version + 1;
      try {
        if (supa.hasSupabase) {
          await supa.saveConfig(tomada.id, newConfig, novaVersao);
        }
        const tomadasNovas = s.tomadas.map((t) =>
          t.id === tomada.id
            ? { ...t, config: newConfig, config_version: novaVersao, updated_at: new Date().toISOString() }
            : t,
        );
        store.set({ tomadas: tomadasNovas });
        showToast('Configuracao salva');
      } catch (err) {
        console.error('[main] saveConfig:', err);
        showToast('Erro ao salvar');
      } finally {
        savingConfig = false;
      }
    },
  });

  // Persiste tweaks
  saveTweaks({
    theme: s.theme,
    accent: s.accent,
    density: s.density,
    scheduleStyle: s.scheduleStyle,
    estadoOverride: s.estadoOverride,
  });
});

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

// === Inicia o carregamento ===
bootstrap();
