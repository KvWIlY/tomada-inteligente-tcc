// =============================================
// Bootstrap do dashboard -- gate de auth + conecta tudo (idioma verde/branco)
// =============================================

import './styles/app.css';
// O wrapper de graficos do Basecoat NAO importa Chart.js: ele consome
// `window.Chart` (dist/js/chart.js e uma IIFE que faz `new Chart` a partir do
// global). Entao o global precisa existir antes de qualquer `basecoat.chart`.
// `chart.js/auto` ja registra controllers/escalas — sem ele seria preciso
// chamar `Chart.register(...registerables)` a mao.
import Chart from 'chart.js/auto';
declare global {
  interface Window { Chart: typeof Chart }
}
window.Chart = Chart;

import { icon } from './icons';
import { consumoTotalAtual, temConexaoRecente } from './derive';
import { createStore } from './store';
import * as api from './services/api';
import type { AppState, Config, Tomada } from './types';
import { renderSidebar } from './views/sidebar';
import { renderOverview } from './views/overview';
import { renderDetail } from './views/detail';
import { renderLogin } from './views/login';
import { renderToast, renderModal, type ToastData } from './views/toast';
import { escapeHtml } from './views/components';

const FORCAR_RESIDENCIA = (import.meta.env.VITE_RESIDENCIA_ID as string | undefined)?.trim() || undefined;

// === Store inicial ===
const store = createStore<AppState>({
  phase: 'checking',
  userEmail: null,
  semBackend: false,
  semResidencia: false,
  residencia: null,
  visao: null,
  tomadas: [],
  medicoesPorTomada: {},
  eventosPorTomada: {},
  selectedId: null,
  navOpen: false,
  loading: true,
  error: null,
  toast: null,
  modal: null,
});

// === Estado de UI local (não reativo) ===
let detailSeg: '24h' | '7d' = '24h';
let cfgDraftId: string | null = null;
let draftSchedule: boolean[] = [];
let draftCritical = false;
let savingConfig = false;

// Auth (tela de login)
let authMode: 'login' | 'signup' = 'login';
let authEmail = '';
let authSenha = '';
let authBusy = false;
let authError: string | null = null;
let authInfo: string | null = null;

// === Toast helper ===
let toastTimer: number | null = null;
function showToast(msg: string, kind: ToastData['kind'] = 'ok'): void {
  store.set({ toast: { msg, kind } });
  if (toastTimer !== null) clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => {
    store.set({ toast: null });
    toastTimer = null;
  }, 3000);
}

function ensureDraft(t: Tomada): void {
  if (cfgDraftId !== t.id) {
    cfgDraftId = t.id;
    draftSchedule = [...t.config.schedule];
    draftCritical = t.config.is_critical;
    detailSeg = '24h';
  }
}
function draftDirty(t: Tomada): boolean {
  return draftCritical !== t.config.is_critical || draftSchedule.some((v, i) => v !== t.config.schedule[i]);
}

// === Carga de dados reais de uma residência ===
async function carregarResidencia(residenciaId: string): Promise<void> {
  const tomadas = await api.fetchTomadas(residenciaId);
  const medicoesPorTomada: Record<string, Awaited<ReturnType<typeof api.fetchMedicoes>>> = {};
  const eventosPorTomada: Record<string, Awaited<ReturnType<typeof api.fetchEventos>>> = {};
  await Promise.all(
    tomadas.map(async (t) => {
      const [meds, evs] = await Promise.all([api.fetchMedicoes(t.id, 24), api.fetchEventos(t.id, 100)]);
      medicoesPorTomada[t.id] = meds;
      eventosPorTomada[t.id] = evs;
    }),
  );
  const visao = await api.fetchResidencia(residenciaId);
  store.set({ tomadas, medicoesPorTomada, eventosPorTomada, visao });
  iniciarPolling(residenciaId);
}

/**
 * O backend proprio nao expoe canal realtime (websocket/push).
 * Como o firmware publica a cada ~60 s, repuxar a residencia a cada 15 s cobre a
 * latencia percebida sem custo relevante numa LAN.
 */
const INTERVALO_POLL_MS = 15_000;
let pollTimer: number | null = null;

function iniciarPolling(residenciaId: string): void {
  if (pollTimer !== null) return;
  pollTimer = window.setInterval(() => {
    // Nao empilha requisicao enquanto o usuario edita a config nem quando a aba
    // esta escondida.
    if (savingConfig || document.hidden) return;
    void atualizarResidencia(residenciaId);
  }, INTERVALO_POLL_MS);
}

function pararPolling(): void {
  if (pollTimer !== null) {
    clearInterval(pollTimer);
    pollTimer = null;
  }
}

/** Refresh silencioso (sem spinner) usado pelo polling. */
async function atualizarResidencia(residenciaId: string): Promise<void> {
  try {
    const tomadas = await api.fetchTomadas(residenciaId);
    const cur = store.get();
    const medicoesPorTomada = { ...cur.medicoesPorTomada };
    const eventosPorTomada = { ...cur.eventosPorTomada };
    // So a tomada aberta precisa de serie fresca; as outras so do estado atual.
    const alvo = cur.selectedId;
    if (alvo) {
      const [meds, evs] = await Promise.all([
        api.fetchMedicoes(alvo, detailSeg === '7d' ? 24 * 7 : 24),
        api.fetchEventos(alvo, 100),
      ]);
      medicoesPorTomada[alvo] = meds;
      eventosPorTomada[alvo] = evs;
    }
    const visao = await api.fetchResidencia(residenciaId);
    store.set({ tomadas, medicoesPorTomada, eventosPorTomada, visao });
  } catch (err) {
    console.error('[main] polling:', err);
  }
}

// Após login: resolve a residência do dono e carrega.
async function bootstrapDono(): Promise<void> {
  store.set({ phase: 'app', loading: true, error: null, semResidencia: false });
  try {
    const minhas = await api.fetchMinhasResidencias();
    let residencia = FORCAR_RESIDENCIA
      ? (minhas.find((r) => r.id === FORCAR_RESIDENCIA) ?? (await api.fetchResidencia(FORCAR_RESIDENCIA)))
      : minhas[0] ?? null;

    if (!residencia) {
      store.set({ loading: false, residencia: null, tomadas: [], semResidencia: true });
      return;
    }
    store.set({ residencia, loading: false });
    await carregarResidencia(residencia.id);
  } catch (err) {
    console.error('[dashboard] bootstrapDono:', err);
    store.set({ loading: false, error: err instanceof Error ? err.message : 'Erro ao carregar dados' });
  }
}

// === Entrada ===
async function iniciar(): Promise<void> {
  // Sem backend não há o que mostrar. O dashboard não inventa dados: informa que
  // falta configuração e para. (Antes havia um gerador de dados falsos aqui; ele
  // mascarava justamente as falhas de integração que só aparecem contra a API real.)
  if (!api.hasBackend) {
    console.error('[dashboard] VITE_BACKEND_URL não configurado');
    store.set({ phase: 'app', semBackend: true, loading: false });
    return;
  }

  // Observa login/logout (a sessão vive em localStorage, gerida por services/api).
  api.onAuth((s) => {
    if (s) {
      store.set({ userEmail: s.email });
      if (store.get().phase !== 'app') void bootstrapDono();
    } else {
      pararPolling();
      store.set({
        phase: 'login', userEmail: null, residencia: null, visao: null, tomadas: [],
        medicoesPorTomada: {}, eventosPorTomada: {}, selectedId: null, semResidencia: false,
      });
    }
  });

  const sessao = await api.getSessao();
  if (sessao) {
    store.set({ userEmail: sessao.email });
    await bootstrapDono();
  } else {
    store.set({ phase: 'login' });
  }
}

// === Ações de tomada ===
const RELE_TOAST: Record<api.ComandoRele, string> = {
  auto: 'Devolvendo ao otimizador · aguardando ESP32',
  usar_agora: 'Religando por 30 min · aguardando ESP32',
  cortar: 'Desligando · aguardando ESP32',
};

async function persistRele(t: Tomada, cmd: api.ComandoRele): Promise<void> {
  const cur = store.get();
  const valor = cmd === 'auto' ? null : cmd === 'cortar';
  store.set({ tomadas: cur.tomadas.map((tt) => (tt.id === t.id ? { ...tt, rele_aberto: valor } : tt)) });
  showToast(RELE_TOAST[cmd]);
  if (api.hasBackend) {
    try {
      await api.setComandoRele(t.id, cmd);
    } catch (err) {
      console.error('[main] setComandoRele:', err);
      store.set({ tomadas: cur.tomadas });
      showToast('Falha ao enviar comando', 'alert');
    }
  }
}

function onComandoRele(t: Tomada, cmd: api.ComandoRele): void {
  const s = store.get();

  // Pré-checagem do teto só para dar resposta imediata: quem realmente decide é o
  // ESP32, que reavalia o P_max com os dados das irmãs e pode negar mesmo assim
  // (registrando o evento `penalidade_uso`).
  if (cmd === 'usar_agora') {
    const pMax = s.residencia?.p_max ?? 2540;
    const projecao = consumoTotalAtual(s.tomadas) + 0.95 * t.con;
    if (projecao > pMax) {
      showToast(`Recusado: ligar excederia o teto (${Math.round(projecao)} W > ${pMax} W)`, 'alert');
      return;
    }
    void persistRele(t, 'usar_agora');
    return;
  }

  if (cmd === 'cortar' && t.config.is_critical) {
    store.set({
      modal: {
        title: 'Desligar tomada crítica?',
        body: `"${t.nome}" está marcada como crítica. Desligá-la manualmente pode interromper um aparelho essencial.`,
        confirmLabel: 'Desligar mesmo assim',
        danger: true,
        onConfirm: () => {
          store.set({ modal: null });
          void persistRele(t, 'cortar');
        },
        onCancel: () => store.set({ modal: null }),
      },
    });
    return;
  }

  void persistRele(t, cmd);
}

async function saveConfig(t: Tomada): Promise<void> {
  savingConfig = true;
  store.set({});
  const novaVersao = t.config_version + 1;
  const novaConfig: Config = { ...t.config, schedule: [...draftSchedule], is_critical: draftCritical };
  try {
    if (api.hasBackend) await api.saveConfig(t.id, novaConfig, novaVersao);
    const cur = store.get();
    store.set({
      tomadas: cur.tomadas.map((tt) =>
        tt.id === t.id ? { ...tt, config: novaConfig, config_version: novaVersao, updated_at: new Date().toISOString() } : tt,
      ),
    });
    cfgDraftId = null;
    showToast(`Configuração salva · v${novaVersao}`);
  } catch (err) {
    console.error('[main] saveConfig:', err);
    showToast('Erro ao salvar configuração', 'alert');
  } finally {
    savingConfig = false;
    store.set({});
  }
}

/** UC26 (005): revoga o hardware. O backend rotaciona a chave da casa na mesma
 *  transação, então as tomadas legítimas reingressam sozinhas na coordenação. */
async function revogar(tomada: Tomada): Promise<void> {
  if (!api.hasBackend) {
    showToast('Revogação exige o backend conectado', 'alert');
    return;
  }
  try {
    await api.revogarDispositivo(tomada.id);
    showToast(`"${tomada.nome}" revogada · chave da casa trocada`);
    // Volta para a lista e recarrega: a tomada revogada some do detalhe.
    store.set({ selectedId: null });
    const s = store.get();
    if (s.residencia) await carregarResidencia(s.residencia.id);
  } catch (err) {
    console.error('[main] revogar:', err);
    showToast('Erro ao revogar dispositivo', 'alert');
  }
}

async function savePMax(w: number): Promise<void> {
  const s = store.get();
  if (!s.residencia) return;
  const ant = s.residencia.p_max;
  store.set({ residencia: { ...s.residencia, p_max: w } });
  if (api.hasBackend) {
    try {
      await api.savePMax(s.residencia.id, w);
      showToast(`Teto do circuito · ${w.toLocaleString('pt-BR')} W`);
    } catch (err) {
      console.error('[main] savePMax:', err);
      store.set({ residencia: { ...s.residencia, p_max: ant } });
      showToast('Erro ao salvar P_max', 'alert');
    }
  } else {
    showToast(`Teto do circuito · ${w.toLocaleString('pt-BR')} W`);
  }
}

// === Renomear tomada (nome exibido na sidebar/topo) ===
async function persistNome(t: Tomada, nome: string): Promise<void> {
  const cur = store.get();
  store.set({ tomadas: cur.tomadas.map((tt) => (tt.id === t.id ? { ...tt, nome } : tt)) });
  showToast(`Renomeada para "${nome}"`);
  if (api.hasBackend) {
    try {
      await api.renameTomada(t.id, nome);
    } catch (err) {
      console.error('[main] renameTomada:', err);
      store.set({ tomadas: cur.tomadas }); // rollback
      showToast('Falha ao renomear', 'alert');
    }
  }
}

function abrirRenomear(t: Tomada): void {
  store.set({
    modal: {
      title: 'Renomear tomada',
      body: '',
      bodyHtml: `
        <p class="modal__body" style="margin-bottom:14px">Esse nome aparece na barra lateral e no topo da página.</p>
        <input class="auth__input" data-role="rename-input" type="text" maxlength="40" autofocus
          value="${escapeHtml(t.nome)}" placeholder="Nome da tomada" style="width:100%" />`,
      confirmLabel: 'Salvar',
      danger: false,
      onConfirm: () => {
        const input = elModal.querySelector<HTMLInputElement>('[data-role="rename-input"]');
        const novo = (input?.value ?? '').trim();
        store.set({ modal: null });
        if (!novo || novo === t.nome) return;
        void persistNome(t, novo);
      },
      onCancel: () => store.set({ modal: null }),
    },
  });
}

async function criarResidenciaDemo(): Promise<void> {
  try {
    const r = await api.criarResidencia('Minha Casa', 3000);
    if (r) {
      showToast('Residência criada');
      await bootstrapDono();
    }
  } catch (err) {
    console.error('[main] criarResidencia:', err);
    showToast('Erro ao criar residência', 'alert');
  }
}

// === Adicionar dispositivo (claim-code de pareamento, UC03) ===
//
// O código é cunhado pelo SERVIDOR (`POST /residencias/{id}/claim-codes`) — o
// navegador não gera nem calcula hash. Não há mais "adoção": o backend só provisiona
// contra um claim válido, então o código é o único caminho.
async function abrirNovoDispositivo(): Promise<void> {
  const s = store.get();
  if (!s.residencia) {
    showToast('Crie uma residência primeiro', 'alert');
    return;
  }
  try {
    const { claim_code: code, expira_em: expiraEm } = await api.criarClaim(s.residencia.id);
    const quando = new Date(expiraEm * 1000).toLocaleTimeString('pt-BR', {
      hour: '2-digit',
      minute: '2-digit',
    });

    store.set({
      modal: {
        title: 'Parear novo dispositivo',
        body: '',
        bodyHtml: `
          <p class="modal__body" style="text-align: justify;">Ligue a tomada e conecte-se à rede Wi-Fi dela. No portal que abrir, informe
            o Wi-Fi da casa e <b>este código</b>. Ela entra direto em <b>${escapeHtml(s.residencia.nome)}</b>.</p>
          <div class="claim-code">
            <code class="claim-code__val" data-role="claim-code">${escapeHtml(code)}</code>
          </div>
          <p class="modal__body" style="font-size:12px;color:#6e6e73;margin-top:10px">Uso único · expira às ${escapeHtml(quando)}.
            Copie ANTES de trocar para a rede da tomada.</p>`,
        confirmLabel: 'Concluído',
        danger: false,
        className: 'modal--device',
        secondaryLabel: 'Copiar código',
        onConfirm: () => store.set({ modal: null }),
        onCancel: () => store.set({ modal: null }),
        onSecondary: () => {
          navigator.clipboard?.writeText(code).then(
            () => showToast('Código copiado'),
            () => showToast('Não foi possível copiar', 'alert'),
          );
        },
      },
    });
  } catch (err) {
    console.error('[main] criarClaim:', err);
    showToast('Erro ao gerar código de pareamento', 'alert');
  }
}

// === Ações de auth ===
function setAuth(patch: Partial<{ mode: 'login' | 'signup'; email: string; senha: string; busy: boolean; error: string | null; info: string | null }>): void {
  if (patch.mode !== undefined) authMode = patch.mode;
  if (patch.email !== undefined) authEmail = patch.email;
  if (patch.senha !== undefined) authSenha = patch.senha;
  if (patch.busy !== undefined) authBusy = patch.busy;
  if (patch.error !== undefined) authError = patch.error;
  if (patch.info !== undefined) authInfo = patch.info;
  store.set({});
}

async function submitAuth(): Promise<void> {
  if (!authEmail || !authSenha) {
    setAuth({ error: 'Preencha e-mail e senha.', info: null });
    return;
  }
  setAuth({ busy: true, error: null, info: null });
  try {
    if (authMode === 'login') {
      await api.entrar(authEmail, authSenha);
      // onAuth dispara o bootstrap.
    } else {
      const { precisaConfirmar } = await api.cadastrar(authEmail, authSenha);
      if (precisaConfirmar) {
        setAuth({ busy: false, mode: 'login', info: 'Conta criada. Confirme pelo e-mail e entre.', senha: '' });
        return;
      }
      // signUp com sessão imediata → onAuth cuida.
    }
  } catch (err) {
    const m = err instanceof Error ? err.message : 'Falha na autenticação';
    setAuth({ busy: false, error: traduzErroAuth(m) });
  }
}

function traduzErroAuth(m: string): string {
  if (/invalid login credentials/i.test(m)) return 'E-mail ou senha incorretos.';
  if (/already registered/i.test(m)) return 'E-mail já cadastrado. Tente entrar.';
  if (/password should be at least/i.test(m)) return 'Senha muito curta (mínimo 6 caracteres).';
  return m;
}

// === Render ===
const elApp = document.querySelector<HTMLElement>('.app')!;
const elSidebar = document.getElementById('sidebar')!;
const elTopbar = document.getElementById('topbar')!;
const elView = document.getElementById('view')!;
const elScrim = document.getElementById('scrim')!;
const elToast = document.getElementById('toast')!;
const elModal = document.getElementById('modal')!;

function renderTopbar(s: AppState, total: number): void {
  const tomada = s.selectedId ? s.tomadas.find((t) => t.id === s.selectedId) ?? null : null;
  const titulo = tomada ? tomada.nome : s.selectedId ? 'Tomada' : 'Visão Geral';
  elTopbar.innerHTML = `
    <div class="topbar">
      <button class="topbar__burger" data-role="burger" aria-label="Menu">
        <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M4 7h16M4 12h16M4 17h16"/></svg>
      </button>
      <span class="topbar__title">${escapeHtml(titulo)}</span>
      ${tomada ? `<button class="icon-btn" data-role="rename" title="Renomear tomada" aria-label="Renomear tomada">${icon('edit', 16)}</button>` : ''}
      <span class="topbar__w tnum">${Math.round(total).toLocaleString('pt-BR')}<small> W</small></span>
    </div>`;
  elTopbar.querySelector('[data-role="burger"]')?.addEventListener('click', () => store.set({ navOpen: true }));
  if (tomada) elTopbar.querySelector('[data-role="rename"]')?.addEventListener('click', () => abrirRenomear(tomada));
}

store.subscribe((s) => {
  renderToast(elToast, s.toast);
  renderModal(elModal, s.modal);

  // Login/checking: sem sidebar — colapsa o grid e some com a barra lateral.
  elApp.classList.toggle('app--auth', s.phase === 'login' || s.phase === 'checking');

  // Tela de login: ocupa tudo, sem sidebar/topbar.
  if (s.phase === 'login') {
    elSidebar.innerHTML = '';
    elTopbar.innerHTML = '';
    elScrim.className = 'scrim';
    renderLogin(elView, {
      mode: authMode, email: authEmail, senha: authSenha, busy: authBusy, error: authError, info: authInfo,
      // Não re-renderiza a cada tecla (senão o <input> é recriado e perde o foco):
      // os campos são uncontrolled; só guardamos o valor para o submit.
      onInput: (campo, valor) => { if (campo === 'email') authEmail = valor; else authSenha = valor; },
      onSubmit: () => void submitAuth(),
      onToggleMode: () => setAuth({ mode: authMode === 'login' ? 'signup' : 'login', error: null, info: null }),
    });
    return;
  }
  if (s.phase === 'checking') {
    elSidebar.innerHTML = '';
    elTopbar.innerHTML = '';
    elView.innerHTML = `<div class="placeholder"><h2>Carregando…</h2></div>`;
    return;
  }

  // App autenticado
  const total = consumoTotalAtual(s.tomadas);
  const online = !s.error && temConexaoRecente(s.tomadas, s.medicoesPorTomada);

  elScrim.className = `scrim${s.navOpen ? ' scrim--show' : ''}`;
  elScrim.onclick = () => store.set({ navOpen: false });

  renderSidebar(elSidebar, {
    tomadas: s.tomadas,
    selectedId: s.selectedId,
    total,
    navOpen: s.navOpen,
    userEmail: s.userEmail,
    onSelect: (id) => store.set({ selectedId: id, navOpen: false }),
    onCloseNav: () => store.set({ navOpen: false }),
    onInfo: (msg) => showToast(msg),
    onRename: (id) => { const t = s.tomadas.find((tt) => tt.id === id); if (t) abrirRenomear(t); },
    onLogout: () => void api.sair(),
    onAddDevice: () => void abrirNovoDispositivo(),
  });
  renderTopbar(s, total);

  if (s.loading) {
    elView.innerHTML = `<div class="placeholder"><h2>Carregando…</h2><p>Buscando dados do servidor.</p></div>`;
    return;
  }
  // Sem VITE_BACKEND_URL não há origem de dados. Estado terminal e explícito.
  if (s.semBackend) {
    elView.innerHTML = `
      <div class="placeholder">
        <h2>Backend não configurado</h2>
        <p>Defina <code>VITE_BACKEND_URL</code> em <code>frontend/.env</code> apontando para a
           API (ex.: <code>http://localhost:8080</code>) e recarregue a página.</p>
        <p style="margin-top:12px;font-size:13px;color:#6e6e73">O dashboard lê exclusivamente
           do backend — não há modo de demonstração com dados fabricados.</p>
      </div>`;
    return;
  }

  if (s.semResidencia) {
    elView.innerHTML = `
      <div class="placeholder">
        <h2>Sua conta ainda não tem residência</h2>
        <p>Crie uma para começar a parear tomadas a ela.</p>
        <button class="l-btn l-btn--green" data-role="criar-res" style="margin-top:16px">Criar minha residência</button>
      </div>`;
    elView.querySelector('[data-role="criar-res"]')?.addEventListener('click', () => void criarResidenciaDemo());
    return;
  }
  if (s.error) {
    elView.innerHTML = `
      <div class="placeholder">
        <h2>Erro: ${escapeHtml(s.error)}</h2>
        <p>Verifique se o backend está no ar e se você tem acesso a esta residência.</p>
      </div>`;
    return;
  }
  if (s.tomadas.length === 0) {
    elView.innerHTML = `
      <div class="placeholder">
        <h2>Nenhuma tomada nesta residência</h2>
        <p>Pareie uma tomada ESP32 à residência ${escapeHtml(s.residencia?.nome ?? '')} (provisionamento do firmware).</p>
      </div>`;
    return;
  }

  if (s.selectedId === null) {
    renderOverview(elView, {
      residencia: s.residencia,
      tomadas: s.tomadas,
      medicoesPorTomada: s.medicoesPorTomada,
      eventosPorTomada: s.eventosPorTomada,
      total,
      score: s.visao?.efficiency_score ?? null,
      online,
      onSelect: (id) => store.set({ selectedId: id }),
    });
    return;
  }

  const tomada = s.tomadas.find((t) => t.id === s.selectedId);
  if (!tomada) {
    store.set({ selectedId: null });
    return;
  }
  ensureDraft(tomada);
  renderDetail(elView, {
    tomada,
    residencia: s.residencia,
    medicoes: s.medicoesPorTomada[tomada.id] ?? [],
    online,
    seg: detailSeg,
    draftSchedule,
    draftCritical,
    dirty: draftDirty(tomada),
    saving: savingConfig,
    onComandoRele: (cmd) => onComandoRele(tomada, cmd),
    onChangeSeg: (sg) => { detailSeg = sg; store.set({}); },
    onScheduleToggle: (h) => { draftSchedule = draftSchedule.map((v, i) => (i === h ? !v : v)); store.set({}); },
    onToggleCritical: () => { draftCritical = !draftCritical; store.set({}); },
    onSaveConfig: () => void saveConfig(tomada),
    onSavePMax: (w) => void savePMax(w),
    onRevogar: () => void revogar(tomada),
  });
});

// === Inicia ===
void iniciar();
