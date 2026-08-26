// =============================================
// Bootstrap do dashboard -- gate de auth + conecta tudo
// =============================================

import './styles/app.css';

// JS do Basecoat. A ORDEM importa: `basecoat.min` cria `window.basecoat` com o
// registro de componentes; os outros tres se registram nele. Trazemos so os
// tres que esta interface usa, e nao o `all.min`, que carregaria acordeao,
// combobox, popover, drawer e select sem nenhum deles aparecer na tela.
//
// O JS do `sidebar` foi DELIBERADAMENTE deixado de fora — ver "Barra lateral".
import 'basecoat-css/basecoat.min';
import 'basecoat-css/toast.min';
import 'basecoat-css/chart.min';

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
import type { AppState, Config, Medicao, Tomada } from './types';
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
/** Série da janela de 7 dias, buscada do backend sob demanda (FR-018).
 *  Vazia enquanto o seletor está em 24 h — nunca preenchida com zeros. */
let medicoes7d: Medicao[] = [];
let cfgDraftId: string | null = null;
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
//
// Sem `setTimeout` proprio: o `toaster` do Basecoat fecha o aviso sozinho e
// pausa a contagem enquanto o ponteiro esta em cima. O estado guarda o ultimo
// aviso so para que `renderToast` saiba se ja o empurrou.
function showToast(msg: string, kind: ToastData['kind'] = 'ok'): void {
  store.set({ toast: { msg, kind } });
}

function ensureDraft(t: Tomada): void {
  if (cfgDraftId !== t.id) {
    cfgDraftId = t.id;
    draftCritical = t.config.is_critical;
    detailSeg = '24h';
    medicoes7d = [];
  }
}
function draftDirty(t: Tomada): boolean {
  return draftCritical !== t.config.is_critical;
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

    // FR-021: `services/api` engole o erro de rede e devolve LISTA VAZIA — e ele
    // esta fora do contrato desta feature, entao a guarda vem para ca. Sem ela,
    // uma queda de Wi-Fi durante o polling escrevia `tomadas: []` no estado e a
    // tela pulava para "Nenhuma tomada nesta residencia", apagando dados que
    // continuavam validos. Uma casa que TINHA tomadas nao passa a nao ter por
    // causa de um GET que falhou; quem remove tomada e a revogacao, que
    // recarrega por outro caminho.
    if (tomadas.length === 0 && cur.tomadas.length > 0) {
      console.warn('[main] polling devolveu lista vazia — mantendo o estado anterior');
      return;
    }

    const medicoesPorTomada = { ...cur.medicoesPorTomada };
    const eventosPorTomada = { ...cur.eventosPorTomada };
    // So a tomada aberta precisa de serie fresca; as outras so do estado atual.
    const alvo = cur.selectedId;
    if (alvo) {
      const [meds, evs] = await Promise.all([api.fetchMedicoes(alvo, 24), api.fetchEventos(alvo, 100)]);
      // Mesma guarda da lista de tomadas: serie vazia so substitui serie vazia.
      // Trocar 288 medicoes por `[]` mandaria o detalhe para o estado "tomada
      // pareada, sem dados ainda" — que e uma afirmacao falsa sobre a tomada.
      if (meds.length > 0 || (medicoesPorTomada[alvo] ?? []).length === 0) medicoesPorTomada[alvo] = meds;
      if (evs.length > 0 || (eventosPorTomada[alvo] ?? []).length === 0) eventosPorTomada[alvo] = evs;
      if (detailSeg === '7d') {
        const sete = await api.fetchMedicoes(alvo, 24 * 7);
        if (sete.length > 0) medicoes7d = sete;
      }
    }
    const visao = await api.fetchResidencia(residenciaId);
    // Mesma razao: `null` aqui e "nao consegui perguntar", nao "nao ha score".
    store.set({ tomadas, medicoesPorTomada, eventosPorTomada, visao: visao ?? cur.visao });
  } catch (err) {
    // FR-021: falha de rede no polling NAO limpa o que ja esta na tela. O erro
    // vai para o console e o indicador de conexao cai sozinho, porque
    // `temConexaoRecente` olha a idade da ultima medicao — nao um flag.
    console.error('[main] polling:', err);
  }
}

/** Troca a janela do histórico REBUSCANDO do backend (FR-005e item 6). */
async function trocarJanela(tomadaId: string, seg: '24h' | '7d'): Promise<void> {
  detailSeg = seg;
  store.set({});
  if (seg === '24h') {
    medicoes7d = [];
    return;
  }
  try {
    medicoes7d = await api.fetchMedicoes(tomadaId, 24 * 7);
  } catch (err) {
    console.error('[main] trocarJanela:', err);
    showToast('Não foi possível carregar os 7 dias', 'alert');
  }
  store.set({});
}

// Após login: resolve a residência do dono e carrega.
async function bootstrapDono(): Promise<void> {
  store.set({ phase: 'app', loading: true, error: null, semResidencia: false });
  try {
    const minhas = await api.fetchMinhasResidencias();
    const residencia = FORCAR_RESIDENCIA
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
      medicoes7d = [];
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
  // `schedule` e `pi_historico` entram pelo espalhamento da config VIGENTE e
  // saem iguais (FR-011): as telas que os editavam morreram na 007, e o campo
  // permanece intacto no contrato por FR-009b/FR-010a.
  const novaConfig: Config = { ...t.config, is_critical: draftCritical };
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

/**
 * Confirmacao da revogacao (FR-005e item 4).
 *
 * Ate a 007 isto era um `window.prompt` pedindo o nome digitado. O dialogo do
 * Basecoat diz o mesmo com o texto todo visivel, e o botao destrutivo nomeia a
 * consequencia — o `prompt` do navegador nao aceita formatacao, nao respeita o
 * tema e, no celular, aparece descolado do resto da interface.
 */
function confirmarRevogacao(t: Tomada): void {
  store.set({
    modal: {
      title: `Revogar "${t.nome}"?`,
      body: '',
      bodyHtml: `
        <p>O ESP32 perde o acesso imediatamente e a chave de coordenação da casa é trocada na mesma
           transação — as outras tomadas recebem a nova sozinhas.</p>
        <p class="mt-3">O histórico de medições é <strong>preservado</strong>. Para religar esta tomada será
           preciso reprovisioná-la com um novo código de pareamento.</p>`,
      confirmLabel: 'Revogar dispositivo',
      danger: true,
      onConfirm: () => {
        store.set({ modal: null });
        void revogar(t);
      },
      onCancel: () => store.set({ modal: null }),
    },
  });
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
        <p class="mb-4">Esse nome aparece na barra lateral e no topo da página.</p>
        <input class="input" data-role="rename-input" data-autofocus type="text" maxlength="40"
          value="${escapeHtml(t.nome)}" placeholder="Nome da tomada" aria-label="Nome da tomada" />`,
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
          <p>Ligue a tomada e conecte-se à rede Wi-Fi dela. No portal que abrir, informe o Wi-Fi da casa
             e <b>este código</b>. Ela entra direto em <b>${escapeHtml(s.residencia.nome)}</b>.</p>
          <code class="mt-3.5 block rounded-xl border border-dashed border-green bg-lav px-4 py-3.5 text-center font-mono
                       text-xl font-bold tracking-widest break-words text-green-ink select-all">${escapeHtml(code)}</code>
          <p class="mt-2.5 text-xs text-ink-faint">Uso único · expira às ${escapeHtml(quando)}.
             Copie ANTES de trocar para a rede da tomada.</p>`,
        confirmLabel: 'Concluído',
        danger: false,
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
const elRoot = document.getElementById('root')!;
const elSidebar = document.getElementById('sidebar')!;
const elTopbar = document.getElementById('topbar')!;
const elView = document.getElementById('view')!;
const elToaster = document.getElementById('toaster')!;
const elModal = document.getElementById('modal')!;
const elTema = document.getElementById('tema')!;

// === Seletor de tema (claro/escuro) ===
//
// Ate 2026-08-26 o tema seguia `prefers-color-scheme` e nao havia seletor
// (FR-002a). O dono pediu o botao com sol e lua, e **claro como padrao** — o
// que significa que a ausencia de escolha NAO consulta mais o sistema
// operacional. A divergencia esta registrada no `RETOMAR.md`.
//
// A troca em si e do `basecoat.theme`: ele poe/tira `dark` no `<html>`, grava
// em `localStorage` sob `themeMode` e dispara `basecoat:themechange` — evento
// que o wrapper de graficos escuta para repintar as cores do Chart.js, que sao
// lidas do CSS uma vez na montagem e nao acompanhariam a troca sozinhas.
//
// O `<html>` ja nasce com a classe certa: o `index.html` a aplica num script
// inline, antes da primeira pintura.
function renderTema(): void {
  const escuro = document.documentElement.classList.contains('dark');
  // Mostra o destino, nao o estado: no claro aparece a lua ("ir para escuro").
  const rotulo = escuro ? 'Mudar para o tema claro' : 'Mudar para o tema escuro';
  elTema.innerHTML = icon(escuro ? 'sun' : 'moon', 18);
  elTema.setAttribute('aria-label', rotulo);
  elTema.setAttribute('title', rotulo);
  elTema.setAttribute('aria-pressed', String(escuro));
}

elTema.addEventListener('click', () => {
  window.basecoat?.theme?.toggle();
  renderTema();
});
renderTema();

// === Barra lateral ===
//
// O JS do Basecoat nao e usado aqui, de proposito. O `handleClick` dele fecha a
// barra quando o alvo do clique nao esta dentro do `<nav>` — e as nossas views
// reescrevem `innerHTML` de forma SINCRONA no proprio handler do clique. Quando
// o evento borbulha ate o `.sidebar`, o botao clicado ja foi desanexado, o
// `nav.contains(target)` da `false`, e o componente conclui que o clique foi
// fora e fecha a barra. Nada a reabria: no desktop ela sumia ate um F5.
//
// A CSS do componente continua valendo — o que passa a ser nosso e so o estado.
//
// No desktop a barra e FIXA: nao ha o que fechar. No celular ela e gaveta, e o
// `navOpen` do estado global (que existia no tipo desde sempre) volta a mandar.
const LARGURA_GAVETA = 768; // o `md` do Tailwind, o mesmo do Basecoat

function ehCelular(): boolean {
  return window.innerWidth < LARGURA_GAVETA;
}

/** Escreve `aria-hidden`/`inert` a partir do estado. Aberta no desktop, sempre. */
function aplicarEstadoDaBarra(navOpen: boolean): void {
  const aberta = !ehCelular() || navOpen;
  elSidebar.setAttribute('aria-hidden', String(!aberta));
  elSidebar.toggleAttribute('inert', !aberta);
}

// O veu escuro e o proprio `.sidebar` no celular: tocar nele (fora do `<nav>`)
// fecha a gaveta.
elSidebar.addEventListener('click', (e) => {
  if (!ehCelular()) return;
  const nav = elSidebar.querySelector('nav');
  if (e.target === elSidebar || (nav && !nav.contains(e.target as Node))) {
    store.set({ navOpen: false });
  }
});

// Girar o aparelho ou redimensionar a janela nao pode deixar a barra escondida
// num desktop nem aberta por acidente num celular.
window.addEventListener('resize', () => aplicarEstadoDaBarra(store.get().navOpen));

/** Envelope das telas de estado — mesmo cartao das demais telas (FR-005d). */
function telaDeEstado(conteudo: string): string {
  return `<div class="grid min-h-full place-items-center p-6"><div class="w-full max-w-lg">${conteudo}</div></div>`;
}

function renderTopbar(s: AppState, total: number): void {
  const tomada = s.selectedId ? s.tomadas.find((t) => t.id === s.selectedId) ?? null : null;
  const titulo = tomada ? tomada.nome : 'Visão Geral';
  elTopbar.innerHTML = `
    <div class="sticky top-0 z-20 flex items-center gap-3.5 border-b border-border bg-card py-3 ps-4 pe-14 md:hidden">
      <button type="button" class="btn" data-variant="outline" data-size="icon" data-role="burger" aria-label="Abrir menu">
        <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M4 7h16M4 12h16M4 17h16"/></svg>
      </button>
      <span class="min-w-0 truncate text-[17px] font-bold text-green-ink">${escapeHtml(titulo)}</span>
      ${tomada ? `<button type="button" class="btn text-ink-faint" data-variant="ghost" data-size="icon-sm" data-role="rename" aria-label="Renomear tomada">${icon('edit', 16)}</button>` : ''}
      <span class="tnum ms-auto text-base font-extrabold tracking-tight">${Math.round(total).toLocaleString('pt-BR')}<small class="text-xs font-bold text-ink-faint"> W</small></span>
    </div>`;
  elTopbar.querySelector('[data-role="burger"]')?.addEventListener('click', () => store.set({ navOpen: !store.get().navOpen }));
  if (tomada) elTopbar.querySelector('[data-role="rename"]')?.addEventListener('click', () => abrirRenomear(tomada));
}

store.subscribe((s) => {
  renderToast(elToaster, s.toast);
  renderModal(elModal, s.modal);

  // Login/checking: sem barra lateral (regra em `tokens.css`).
  const semSidebar = s.phase === 'login' || s.phase === 'checking';
  elRoot.toggleAttribute('data-auth', semSidebar);

  // Tela de login: ocupa tudo, sem sidebar/topbar.
  if (s.phase === 'login') {
    elSidebar.innerHTML = '';
    elTopbar.innerHTML = '';
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
    elView.innerHTML = telaDeEstado(`
      <div class="card">
        <header><h2>Carregando…</h2></header>
        <section class="space-y-3">
          <div class="skeleton h-4 w-2/3"></div>
          <div class="skeleton h-4 w-full"></div>
          <div class="skeleton h-24 w-full"></div>
        </section>
      </div>`);
    return;
  }

  // App autenticado
  const total = consumoTotalAtual(s.tomadas);
  const online = !s.error && temConexaoRecente(s.tomadas, s.medicoesPorTomada);

  aplicarEstadoDaBarra(s.navOpen);
  renderSidebar(elSidebar, {
    tomadas: s.tomadas,
    selectedId: s.selectedId,
    total,
    userEmail: s.userEmail,
    onSelect: (id) => store.set({ selectedId: id, navOpen: false }),
    onInfo: (msg) => showToast(msg),
    onRename: (id) => { const t = s.tomadas.find((tt) => tt.id === id); if (t) abrirRenomear(t); },
    onLogout: () => void api.sair(),
    onAddDevice: () => void abrirNovoDispositivo(),
  });
  renderTopbar(s, total);

  if (s.loading) {
    elView.innerHTML = telaDeEstado(`
      <div class="card">
        <header><h2>Carregando…</h2><p data-description>Buscando dados do servidor.</p></header>
        <section class="space-y-3">
          <div class="skeleton h-4 w-1/2"></div>
          <div class="skeleton h-24 w-full"></div>
          <div class="skeleton h-4 w-3/4"></div>
        </section>
      </div>`);
    return;
  }
  // Sem VITE_BACKEND_URL não há origem de dados. Estado terminal e explícito.
  if (s.semBackend) {
    elView.innerHTML = telaDeEstado(`
      <div class="alert" data-variant="destructive" role="alert">
        ${icon('warn', 16)}
        <h2>Backend não configurado</h2>
        <section>
          <p>Defina <code class="font-mono">VITE_BACKEND_URL</code> em <code class="font-mono">frontend/.env</code> apontando
             para a API (ex.: <code class="font-mono">http://localhost:8080</code>) e recarregue a página.</p>
          <p>O dashboard lê exclusivamente do backend — não há modo de demonstração com dados fabricados.</p>
        </section>
      </div>`);
    return;
  }

  if (s.semResidencia) {
    elView.innerHTML = telaDeEstado(`
      <div class="card">
        <section class="empty py-10">
          <header>
            <figure class="mb-4 grid size-14 place-items-center rounded-2xl bg-lav text-ink-faint">${icon('plus', 26)}</figure>
            <h2 class="text-xl font-bold tracking-tight">Sua conta ainda não tem residência</h2>
            <p class="mt-2 text-sm text-ink-soft">Crie uma para começar a parear tomadas a ela.</p>
          </header>
          <footer class="mt-5"><button type="button" class="btn" data-role="criar-res">Criar minha residência</button></footer>
        </section>
      </div>`);
    elView.querySelector('[data-role="criar-res"]')?.addEventListener('click', () => void criarResidenciaDemo());
    return;
  }
  if (s.error) {
    elView.innerHTML = telaDeEstado(`
      <div class="alert" data-variant="destructive" role="alert">
        ${icon('warn', 16)}
        <h2>Erro ao carregar</h2>
        <section>
          <p>${escapeHtml(s.error)}</p>
          <p>Verifique se o backend está no ar e se você tem acesso a esta residência.</p>
        </section>
      </div>`);
    return;
  }
  if (s.tomadas.length === 0) {
    elView.innerHTML = telaDeEstado(`
      <div class="card">
        <section class="empty py-10">
          <header>
            <figure class="mb-4 grid size-14 place-items-center rounded-2xl bg-lav text-ink-faint">${icon('plug', 26)}</figure>
            <h2 class="text-xl font-bold tracking-tight">Nenhuma tomada nesta residência</h2>
            <p class="mt-2 text-sm text-ink-soft">Pareie uma tomada ESP32 a ${escapeHtml(s.residencia?.nome ?? 'esta residência')} pelo
               portal Wi-Fi do firmware, usando um código de pareamento.</p>
          </header>
          <footer class="mt-5"><button type="button" class="btn" data-role="novo-disp">Gerar código de pareamento</button></footer>
        </section>
      </div>`);
    elView.querySelector('[data-role="novo-disp"]')?.addEventListener('click', () => void abrirNovoDispositivo());
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
  const medicoes = s.medicoesPorTomada[tomada.id] ?? [];
  renderDetail(elView, {
    tomada,
    residencia: s.residencia,
    medicoes,
    // Enquanto os 7 dias não chegam, segue plotando as 24 h já em mãos — nunca
    // uma série de zeros (CHK032).
    medicoesJanela: detailSeg === '7d' && medicoes7d.length > 0 ? medicoes7d : medicoes,
    online,
    seg: detailSeg,
    draftCritical,
    dirty: draftDirty(tomada),
    saving: savingConfig,
    onComandoRele: (cmd) => onComandoRele(tomada, cmd),
    onChangeSeg: (sg) => void trocarJanela(tomada.id, sg),
    onToggleCritical: () => { draftCritical = !draftCritical; store.set({}); },
    onSaveConfig: () => void saveConfig(tomada),
    onSavePMax: (w) => void savePMax(w),
    onRevogar: () => confirmarRevogacao(tomada),
  });
});

// === Inicia ===
void iniciar();
