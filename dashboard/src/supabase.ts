// =============================================
// Cliente Supabase + queries + realtime
// =============================================
//
// Le credenciais do .env (build-time via import.meta.env).
// Tabelas adaptadas ao SCHEMA REAL do firmware (UUIDs + bigint timestamp).

import { createClient, type RealtimeChannel, type SupabaseClient } from '@supabase/supabase-js';
import type { Config, Evento, Medicao, Residencia, Tomada } from './types';

const url = import.meta.env.VITE_SUPABASE_URL as string | undefined;
const key = import.meta.env.VITE_SUPABASE_ANON_KEY as string | undefined;

export const hasSupabase: boolean = Boolean(url && key);

let client: SupabaseClient | null = null;
if (hasSupabase) {
  client = createClient(url!, key!);
}

export const supabase: SupabaseClient | null = client;

// =============================================
// Auth (login do dono — Supabase Auth)
// =============================================
//
// A sessao do dono e persistida em localStorage pelo supabase-js; toda query passa a
// levar o JWT `authenticated`, e a RLS (`usr_*`) libera so as linhas de owner_id = auth.uid().

export interface SessaoDono {
  userId: string;
  email: string | null;
}

function mapSession(s: { user: { id: string; email?: string } } | null): SessaoDono | null {
  if (!s) return null;
  return { userId: s.user.id, email: s.user.email ?? null };
}

export async function getSessao(): Promise<SessaoDono | null> {
  if (!client) return null;
  const { data } = await client.auth.getSession();
  return mapSession(data.session as never);
}

/** Observa mudancas de auth (login/logout). Retorna unsubscribe. */
export function onAuth(cb: (s: SessaoDono | null) => void): () => void {
  if (!client) return () => {};
  const { data } = client.auth.onAuthStateChange((_evt, session) => cb(mapSession(session as never)));
  return () => data.subscription.unsubscribe();
}

export async function entrar(email: string, senha: string): Promise<void> {
  if (!client) throw new Error('Supabase nao configurado');
  const { error } = await client.auth.signInWithPassword({ email, password: senha });
  if (error) throw error;
}

export async function cadastrar(email: string, senha: string): Promise<{ precisaConfirmar: boolean }> {
  if (!client) throw new Error('Supabase nao configurado');
  const { data, error } = await client.auth.signUp({ email, password: senha });
  if (error) throw error;
  // Sem sessao apos signUp => projeto exige confirmacao de e-mail.
  return { precisaConfirmar: !data.session };
}

export async function sair(): Promise<void> {
  if (!client) return;
  await client.auth.signOut();
}

// =============================================
// Queries
// =============================================

function mapResidencia(res: Record<string, unknown>): Residencia {
  return {
    id: res.id as string,
    nome: res.nome as string,
    p_max: Number(res.p_max ?? 2540),
    created_at: (res.criado_em ?? res.created_at ?? new Date().toISOString()) as string,
  };
}

/** Residencias do dono logado (RLS filtra por owner_id = auth.uid()). */
export async function fetchMinhasResidencias(): Promise<Residencia[]> {
  if (!client) return [];
  const { data, error } = await client.from('residencias').select('*').order('criado_em');
  if (error) {
    console.error('[supabase] fetchMinhasResidencias:', error);
    return [];
  }
  return (data ?? []).map((r) => mapResidencia(r as Record<string, unknown>));
}

/** Cria uma residencia para o dono logado (owner_id = auth.uid(), exigido pela RLS). */
export async function criarResidencia(nome: string, pMax = 3000): Promise<Residencia | null> {
  if (!client) return null;
  const { data: u } = await client.auth.getUser();
  const ownerId = u.user?.id;
  if (!ownerId) throw new Error('sem sessao');
  const { data, error } = await client
    .from('residencias')
    .insert({ nome, p_max: pMax, owner_id: ownerId })
    .select('*')
    .single();
  if (error) throw error;
  return mapResidencia(data as Record<string, unknown>);
}

export async function fetchResidencia(residenciaId: string): Promise<Residencia | null> {
  if (!client) return null;
  const { data, error } = await client.from('residencias').select('*').eq('id', residenciaId).single();
  if (error) {
    console.error('[supabase] fetchResidencia:', error);
    return null;
  }
  return mapResidencia(data as Record<string, unknown>);
}

export async function fetchTomadas(residenciaId: string): Promise<Tomada[]> {
  if (!client) return [];
  const { data, error } = await client
    .from('tomadas')
    .select('*')
    .eq('residencia_id', residenciaId)
    .order('nome');
  if (error) {
    console.error('[supabase] fetchTomadas:', error);
    return [];
  }
  return (data ?? []).map(normalizeTomada);
}

export async function fetchMedicoes(tomadaId: string, horas = 24): Promise<Medicao[]> {
  if (!client) return [];
  const desdeUnix = Math.floor(Date.now() / 1000) - horas * 3600;
  const { data, error } = await client
    .from('medicoes')
    .select('*')
    .eq('tomada_id', tomadaId)
    .gte('timestamp', desdeUnix)
    .order('timestamp', { ascending: true })
    .limit(2000);
  if (error) {
    console.error('[supabase] fetchMedicoes:', error);
    return [];
  }
  return (data ?? []) as Medicao[];
}

export async function fetchEventos(tomadaId: string, limite = 100): Promise<Evento[]> {
  if (!client) return [];
  const { data, error } = await client
    .from('eventos')
    .select('*')
    .eq('tomada_id', tomadaId)
    .order('timestamp', { ascending: false })
    .limit(limite);
  if (error) {
    console.error('[supabase] fetchEventos:', error);
    return [];
  }
  return (data ?? []) as Evento[];
}

// =============================================
// Mutations
// =============================================

/** Salva P_max na residencia (capacidade do circuito). */
export async function savePMax(residenciaId: string, pMax: number): Promise<void> {
  if (!client) throw new Error('Supabase nao configurado');
  const { error } = await client.from('residencias').update({ p_max: pMax }).eq('id', residenciaId);
  if (error) throw error;
}

/** Liga/desliga manual: escreve `rele_aberto` na tomada. ESP32 le e aplica. */
export async function setReleAberto(tomadaId: string, aberto: boolean): Promise<void> {
  if (!client) throw new Error('Supabase nao configurado');
  const { error } = await client.from('tomadas').update({ rele_aberto: aberto }).eq('id', tomadaId);
  if (error) throw error;
}

/** Renomeia a tomada (coluna `nome` — o rotulo exibido na sidebar/topo). A RLS
 *  `usr_tomadas_all` exige que a residencia seja do dono (owner_id = auth.uid()). */
export async function renameTomada(tomadaId: string, nome: string): Promise<void> {
  if (!client) throw new Error('Supabase nao configurado');
  const { error } = await client.from('tomadas').update({ nome }).eq('id', tomadaId);
  if (error) throw error;
}

/** Salva nova config no JSONB `config` (schema real) + bump de versao + evento na timeline. */
export async function saveConfig(
  tomadaId: string,
  config: Config,
  novaVersao: number,
): Promise<void> {
  if (!client) throw new Error('Supabase nao configurado');
  // `config` e uma coluna JSONB (espelha o struct Tomada do firmware), NAO colunas flat.
  const configJson: Record<string, unknown> = {
    is_critical: config.is_critical,
    w_i: config.w_i,
    t_limite: config.t_limite,
    schedule: config.schedule,
    pi_historico: config.pi_historico,
  };
  if (config.p_limiar_detectado_auto !== undefined) {
    configJson.p_limiar_detectado_auto = config.p_limiar_detectado_auto;
  }
  const { error: e1 } = await client
    .from('tomadas')
    .update({ config: configJson, config_version: novaVersao })
    .eq('id', tomadaId);
  if (e1) throw e1;

  const { error: e2 } = await client.from('eventos').insert({
    tomada_id: tomadaId,
    timestamp: Math.floor(Date.now() / 1000),
    tipo: 'config',
    severidade: 'info',
    titulo: 'Configuracao atualizada',
    detalhes: `config_version → v${novaVersao}`,
  });
  if (e2) console.error('[supabase] evento config insert:', e2);
}

// =============================================
// Provisionamento (claim-codes de pareamento)
// =============================================

/** sha256 hex minúsculo — DEVE casar com o `sha256Hex` da Edge Function `provisionar`
 *  e do `scripts/criar-claim.ps1`. Requer contexto seguro (localhost/https). */
async function sha256Hex(s: string): Promise<string> {
  const buf = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(s));
  return Array.from(new Uint8Array(buf))
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('');
}

/** Cria um claim-code de uso único para a residência (guarda só o hash). A RLS
 *  `usr_claims_all` exige `owner_id = auth.uid()`. Retorna o instante de expiração (ISO). */
export async function criarClaim(residenciaId: string, code: string, horas = 6): Promise<string> {
  if (!client) throw new Error('Supabase nao configurado');
  const codeHash = await sha256Hex(code);
  const expiraEm = new Date(Date.now() + horas * 3_600_000).toISOString();
  const { error } = await client
    .from('provisioning_claims')
    .insert({ residencia_id: residenciaId, code_hash: codeHash, expira_em: expiraEm });
  if (error) throw error;
  return expiraEm;
}

// =============================================
// Realtime subscriptions
// =============================================

export function subscribeMedicoes(
  tomadaId: string,
  cb: (m: Medicao) => void,
): RealtimeChannel | null {
  if (!client) return null;
  const ch = client
    .channel(`medicoes:${tomadaId}`)
    .on(
      'postgres_changes',
      { event: 'INSERT', schema: 'public', table: 'medicoes', filter: `tomada_id=eq.${tomadaId}` },
      (payload) => cb(payload.new as Medicao),
    )
    .subscribe();
  return ch;
}

export function subscribeEventos(
  tomadaId: string,
  cb: (e: Evento) => void,
): RealtimeChannel | null {
  if (!client) return null;
  const ch = client
    .channel(`eventos:${tomadaId}`)
    .on(
      'postgres_changes',
      { event: 'INSERT', schema: 'public', table: 'eventos', filter: `tomada_id=eq.${tomadaId}` },
      (payload) => cb(payload.new as Evento),
    )
    .subscribe();
  return ch;
}

export function subscribeTomadas(
  residenciaId: string,
  cb: (t: Tomada) => void,
): RealtimeChannel | null {
  if (!client) return null;
  const ch = client
    .channel(`tomadas:${residenciaId}`)
    .on(
      'postgres_changes',
      { event: 'UPDATE', schema: 'public', table: 'tomadas', filter: `residencia_id=eq.${residenciaId}` },
      (payload) => cb(normalizeTomada(payload.new)),
    )
    .subscribe();
  return ch;
}

// =============================================
// Helpers
// =============================================

/** Normaliza linha do Postgres -> tipo Tomada.
 *  Schema v2: config e colunas flat, nao JSONB. */
function normalizeTomada(raw: Record<string, unknown>): Tomada {
  // Schema v2: campos flat. Fallback: tenta ler JSONB config antigo se existir.
  const legacyCfg = raw.config as Record<string, unknown> | null;

  let schedule = (raw.schedule ?? legacyCfg?.schedule ?? []) as boolean[];
  if (!Array.isArray(schedule) || schedule.length === 0) {
    schedule = Array(24).fill(true);
  } else if (schedule.length !== 24) {
    schedule = [...schedule, ...Array(24 - schedule.length).fill(true)].slice(0, 24);
  }

  let piHistorico = (raw.pi_historico ?? legacyCfg?.pi_historico ?? Array(24).fill(0.5)) as number[];
  if (!Array.isArray(piHistorico) || piHistorico.length !== 24) {
    piHistorico = [...(piHistorico || []), ...Array(24).fill(0.5)].slice(0, 24);
  }

  const cfg: Config = {
    p_limiar_detectado_auto: (raw.p_limiar_detectado_auto ?? legacyCfg?.p_limiar_detectado_auto ?? undefined) as number | undefined,
    t_limite: Number(raw.t_limite ?? legacyCfg?.t_limite ?? 600),
    is_critical: Boolean(raw.is_critical ?? legacyCfg?.is_critical ?? false),
    schedule,
    w_i: Number(raw.w_i ?? legacyCfg?.w_i ?? 10),
    pi_historico: piHistorico,
  };

  return {
    id: raw.id as string,
    residencia_id: raw.residencia_id as string,
    nome: raw.nome as string,
    localizacao: (raw.localizacao as string | null) ?? null,
    icone: ((raw.icone as string) || 'plug') as Tomada['icone'],
    config: cfg,
    config_version: Number(raw.config_version ?? 1),
    updated_at: (raw.updated_at as string) ?? new Date().toISOString(),
    cstd: Number(raw.cstd ?? 0),
    con: Number(raw.con ?? 0),
    rele_aberto: Boolean(raw.rele_aberto ?? false),
    estado: ((raw.estado as string) || 'OFF') as Tomada['estado'],
    p_medida_atual: Number(raw.p_medida_atual ?? 0),
    mac: (raw.mac as string | undefined) ?? undefined,
  };
}
