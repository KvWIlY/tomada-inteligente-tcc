// =============================================
// Cliente da API do backend proprio (feature 004)
// =============================================
//
// Todo acesso a dados passa por HTTP autenticado com o JWT
// de USUARIO emitido por `POST /auth/login` - nao existe mais acesso direto ao banco
// nem RLS: a autorizacao e default-deny no backend (Constituicao v2.0.0, Princ. VI/VIII).
//
// Contrato: specs/004-backend-actix-mysql/contracts/backend-api.md

import type { Config, Evento, Medicao, Residencia, Tomada } from '../types';

const BASE = ((import.meta.env.VITE_BACKEND_URL as string | undefined) ?? '').replace(/\/+$/, '');

export const hasBackend: boolean = Boolean(BASE);

// =============================================
// Sessao (JWT de usuario em localStorage)
// =============================================
//
// Este modulo e o dono da sessao: guarda o token, o `exp` e o e-mail (so p/ exibir)
// - nunca a senha.

const CHAVE_SESSAO = 'tcc.sessao';

export interface SessaoDono {
  userId: string;
  email: string | null;
}

interface SessaoGravada {
  jwt: string;
  jwtExp: number;
  email: string;
}

const ouvintes = new Set<(s: SessaoDono | null) => void>();

function lerSessao(): SessaoGravada | null {
  try {
    const cru = localStorage.getItem(CHAVE_SESSAO);
    if (!cru) return null;
    const s = JSON.parse(cru) as SessaoGravada;
    // Token expirado nao serve para nada: trata como deslogado.
    if (!s.jwt || s.jwtExp * 1000 <= Date.now()) {
      localStorage.removeItem(CHAVE_SESSAO);
      return null;
    }
    return s;
  } catch {
    return null;
  }
}

function gravarSessao(s: SessaoGravada | null): void {
  if (s) localStorage.setItem(CHAVE_SESSAO, JSON.stringify(s));
  else localStorage.removeItem(CHAVE_SESSAO);
  const pub = s ? paraSessaoDono(s) : null;
  for (const cb of ouvintes) cb(pub);
}

/** `sub` do JWT - o backend nao tem endpoint de "quem sou eu", entao lemos do payload. */
function paraSessaoDono(s: SessaoGravada): SessaoDono {
  let userId = '';
  try {
    const payload = JSON.parse(atob(s.jwt.split('.')[1] ?? '')) as { sub?: string };
    userId = payload.sub ?? '';
  } catch {
    /* token malformado: segue sem userId (a autorizacao real e no servidor) */
  }
  return { userId, email: s.email };
}

export async function getSessao(): Promise<SessaoDono | null> {
  const s = lerSessao();
  return s ? paraSessaoDono(s) : null;
}

/** Observa login/logout. Retorna unsubscribe. */
export function onAuth(cb: (s: SessaoDono | null) => void): () => void {
  ouvintes.add(cb);
  return () => ouvintes.delete(cb);
}

export async function entrar(email: string, senha: string): Promise<void> {
  const r = await req<{ jwt: string; jwt_exp: number }>('POST', '/auth/login', { email, senha }, false);
  gravarSessao({ jwt: r.jwt, jwtExp: r.jwt_exp, email });
}

/** Cria a conta e ja entra - o backend nao exige confirmacao de e-mail. */
export async function cadastrar(email: string, senha: string): Promise<{ precisaConfirmar: boolean }> {
  await req<{ id: string }>('POST', '/auth/register', { email, senha }, false);
  await entrar(email, senha);
  return { precisaConfirmar: false };
}

export async function sair(): Promise<void> {
  gravarSessao(null);
}

// =============================================
// Transporte
// =============================================

export class ErroApi extends Error {
  readonly status: number;
  readonly codigo: string;

  constructor(status: number, codigo: string, msg: string) {
    super(msg);
    this.name = 'ErroApi';
    this.status = status;
    this.codigo = codigo;
  }
}

async function req<T>(
  metodo: string,
  caminho: string,
  corpo?: unknown,
  autenticado = true,
): Promise<T> {
  if (!BASE) throw new Error('VITE_BACKEND_URL nao configurado');
  const headers: Record<string, string> = { 'Content-Type': 'application/json' };
  if (autenticado) {
    const s = lerSessao();
    if (!s) throw new ErroApi(401, 'nao_autorizado', 'sessao expirada');
    headers.Authorization = `Bearer ${s.jwt}`;
  }

  const resp = await fetch(`${BASE}${caminho}`, {
    method: metodo,
    headers,
    body: corpo === undefined ? undefined : JSON.stringify(corpo),
  });

  if (!resp.ok) {
    // 401 num endpoint autenticado = token morto: derruba a sessao para a UI
    // voltar ao login em vez de ficar tentando em silencio.
    if (resp.status === 401 && autenticado) gravarSessao(null);
    let codigo = 'erro';
    let msg = `HTTP ${resp.status}`;
    try {
      const j = (await resp.json()) as { erro?: string; msg?: string };
      codigo = j.erro ?? codigo;
      msg = j.msg ?? msg;
    } catch {
      /* corpo nao-JSON: mantem a mensagem generica */
    }
    throw new ErroApi(resp.status, codigo, msg);
  }
  if (resp.status === 204) return undefined as T;
  return (await resp.json()) as T;
}

// =============================================
// Leitura
// =============================================

interface ResidenciaBruta {
  id: string;
  nome: string;
  p_max: number;
  criado_em: string;
}

function mapResidencia(r: ResidenciaBruta): Residencia {
  return {
    id: r.id,
    nome: r.nome,
    p_max: Number(r.p_max ?? 2540),
    created_at: r.criado_em ?? new Date().toISOString(),
  };
}

/** Residencias do dono logado (o backend filtra por `owner_id` do claim). */
export async function fetchMinhasResidencias(): Promise<Residencia[]> {
  try {
    const rs = await req<ResidenciaBruta[]>('GET', '/residencias');
    return rs.map(mapResidencia);
  } catch (err) {
    console.error('[api] fetchMinhasResidencias:', err);
    return [];
  }
}

export async function criarResidencia(nome: string, pMax = 3000): Promise<Residencia | null> {
  const r = await req<ResidenciaBruta>('POST', '/residencias', { nome, p_max: pMax });
  return mapResidencia(r);
}

/** Detalhe + visao geral (consumo instantaneo e score vem calculados do servidor). */
export interface VisaoGeral extends Residencia {
  tomadas: number;
  consumo_total_atual: number;
  efficiency_score: number | null;
}

export async function fetchResidencia(residenciaId: string): Promise<VisaoGeral | null> {
  try {
    const r = await req<
      ResidenciaBruta & { tomadas: number; consumo_total_atual: number; efficiency_score: number | null }
    >('GET', `/residencias/${residenciaId}`);
    return {
      ...mapResidencia(r),
      tomadas: r.tomadas,
      consumo_total_atual: r.consumo_total_atual,
      efficiency_score: r.efficiency_score,
    };
  } catch (err) {
    console.error('[api] fetchResidencia:', err);
    return null;
  }
}

export async function fetchTomadas(residenciaId: string): Promise<Tomada[]> {
  try {
    const ts = await req<Record<string, unknown>[]>('GET', `/residencias/${residenciaId}/tomadas`);
    return ts.map(normalizeTomada);
  } catch (err) {
    console.error('[api] fetchTomadas:', err);
    return [];
  }
}

export async function fetchMedicoes(tomadaId: string, horas = 24): Promise<Medicao[]> {
  const janela = horas > 24 ? '7d' : '24h';
  try {
    const ms = await req<Record<string, unknown>[]>(
      'GET',
      `/tomadas/${tomadaId}/medicoes?janela=${janela}`,
    );
    return ms.map(normalizeMedicao);
  } catch (err) {
    console.error('[api] fetchMedicoes:', err);
    return [];
  }
}

export async function fetchEventos(tomadaId: string, _limite = 100): Promise<Evento[]> {
  try {
    const es = await req<Record<string, unknown>[]>('GET', `/tomadas/${tomadaId}/eventos`);
    return es.map(normalizeEvento);
  } catch (err) {
    console.error('[api] fetchEventos:', err);
    return [];
  }
}

// =============================================
// Escrita
// =============================================

/** P_max da residencia (teto eletrico - safety-critical, UC07). */
export async function savePMax(residenciaId: string, pMax: number): Promise<void> {
  await req('PATCH', `/residencias/${residenciaId}`, { p_max: pMax });
}

/** As tres intencoes do dono sobre o rele. Sao coisas diferentes, e nao graus da
 *  mesma coisa - por isso um tipo fechado em vez de um boolean. */
export type ComandoRele = 'auto' | 'usar_agora' | 'cortar';

/** Comando manual (UC09). O no puxa em `GET /config` no proximo tick de nuvem.
 *  `auto` manda `null` EXPLICITO no corpo: o backend distingue campo ausente
 *  ("nao mexi") de `null` ("volta ao automatico"). */
export async function setComandoRele(tomadaId: string, cmd: ComandoRele): Promise<void> {
  const rele_aberto = cmd === 'auto' ? null : cmd === 'cortar';
  await req('PATCH', `/tomadas/${tomadaId}`, { rele_aberto });
}

export async function renameTomada(tomadaId: string, nome: string): Promise<void> {
  await req('PATCH', `/tomadas/${tomadaId}`, { nome });
}

/**
 * Salva a config do modelo (UC06). O **servidor** incrementa `config_version` - o
 * dashboard nao cunha versao (fazer isso no cliente permitia corrida entre abas).
 * Por isso `novaVersao` e ignorado; fica na assinatura so para nao quebrar chamadas.
 */
export async function saveConfig(
  tomadaId: string,
  config: Config,
  _novaVersao?: number,
): Promise<void> {
  const configJson: Record<string, unknown> = {
    is_critical: config.is_critical,
    w_i: config.w_i,
    t_limite: config.t_limite,
    schedule: config.schedule,
  };
  if (config.p_limiar_detectado_auto !== undefined) {
    configJson.p_limiar_detectado_auto = config.p_limiar_detectado_auto;
  }
  await req('PATCH', `/tomadas/${tomadaId}`, { config: configJson });
}

/** Revoga o dispositivo (UC26): ele para de renovar e o JWT vigente morre em <= TTL. */
export async function revogarDispositivo(tomadaId: string): Promise<void> {
  await req('DELETE', `/tomadas/${tomadaId}`);
}

// =============================================
// Provisionamento (claim-codes de pareamento)
// =============================================

export interface ClaimCriado {
  claim_code: string;
  /** Epoch (s) em que o codigo expira. */
  expira_em: number;
}

/**
 * Gera um claim-code de uso unico para a residencia (UC03).
 *
 * O **codigo e cunhado pelo servidor**, nao pelo navegador: o dashboard nao calcula
 * hash nem ve o segredo. O dono digita o codigo no Portal AP da tomada.
 */
export async function criarClaim(residenciaId: string): Promise<ClaimCriado> {
  return req<ClaimCriado>('POST', `/residencias/${residenciaId}/claim-codes`);
}

// =============================================
// Helpers de normalizacao
// =============================================

/** MySQL manda `TINYINT(1)` como 0/1; o resto do app quer boolean. */
function comoBool(v: unknown): boolean {
  return v === true || v === 1 || v === '1';
}

/** Idem, mas preservando `null` - em `tomadas.rele_aberto` o nulo NAO e "false":
 *  e o modo AUTOMATICO, em que quem manda no rele e o MILP (migration 0002). */
function comoBoolOuNulo(v: unknown): boolean | null {
  return v === null || v === undefined ? null : comoBool(v);
}

/**
 * Linha de `tomadas` -> tipo Tomada. Os parametros do modelo vivem no JSON `config`;
 * `localizacao`/`icone` NAO existem no banco - sao defaults locais da UI.
 */
function normalizeTomada(raw: Record<string, unknown>): Tomada {
  const cfgJson = (raw.config ?? {}) as Record<string, unknown>;

  let schedule = (cfgJson.schedule ?? []) as boolean[];
  if (!Array.isArray(schedule) || schedule.length === 0) {
    schedule = Array(24).fill(true);
  } else if (schedule.length !== 24) {
    schedule = [...schedule, ...Array(24 - schedule.length).fill(true)].slice(0, 24);
  }

  const cfg: Config = {
    p_limiar_detectado_auto: (cfgJson.p_limiar_detectado_auto ?? undefined) as number | undefined,
    t_limite: Number(cfgJson.t_limite ?? 600),
    is_critical: Boolean(cfgJson.is_critical ?? false),
    schedule,
    w_i: Number(cfgJson.w_i ?? 10),
  };

  return {
    id: raw.id as string,
    residencia_id: raw.residencia_id as string,
    nome: raw.nome as string,
    localizacao: null,
    icone: 'plug',
    config: cfg,
    config_version: Number(raw.config_version ?? 0),
    updated_at: (raw.criado_em as string) ?? new Date().toISOString(),
    cstd: Number(raw.cstd ?? 0),
    con: Number(raw.con ?? 0),
    rele_aberto: comoBoolOuNulo(raw.rele_aberto),
    estado: ((raw.estado as string) || 'OFF') as Tomada['estado'],
    p_medida_atual: Number(raw.p_medida_atual ?? 0),
    mac: (raw.mac as string | undefined) ?? undefined,
  };
}

function normalizeMedicao(raw: Record<string, unknown>): Medicao {
  return {
    id: Number(raw.id),
    tomada_id: raw.tomada_id as string,
    timestamp: Number(raw.timestamp),
    p_medida: Number(raw.p_medida ?? 0),
    // `?? null` e nao `?? 0`: ausente significa desconhecido (medicao antiga), e um
    // zero aqui viraria "fator de potencia pessimo" num grafico.
    fator_potencia: raw.fator_potencia == null ? null : Number(raw.fator_potencia),
    estado: ((raw.estado as string) || 'OFF') as Medicao['estado'],
    rele_aberto: comoBool(raw.rele_aberto),
    created_at: (raw.criado_em as string) ?? '',
  };
}

function normalizeEvento(raw: Record<string, unknown>): Evento {
  return {
    id: Number(raw.id),
    tomada_id: raw.tomada_id as string,
    timestamp: Number(raw.timestamp),
    tipo: raw.tipo as Evento['tipo'],
    severidade: raw.severidade as Evento['severidade'],
    titulo: raw.titulo as string,
    detalhes: (raw.detalhes as string | null) ?? null,
    created_at: (raw.criado_em as string) ?? '',
  };
}
