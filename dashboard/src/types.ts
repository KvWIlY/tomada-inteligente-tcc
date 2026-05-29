// =============================================
// Tipos do dashboard
// =============================================
//
// Refletem o SCHEMA REAL do Supabase (UUIDs + bigint timestamp).
// Adaptado do HANDOFF.md para o que o firmware esta efetivamente enviando.

export type Estado = 'ON' | 'STD' | 'OFF';
export type Icone = 'printer' | 'tv' | 'fridge' | 'charger' | 'plug';
export type ScheduleStyle = 'grid' | 'donut' | 'bar';
export type Theme = 'light' | 'dark';
export type Accent = 'violet' | 'indigo' | 'teal' | 'orange' | 'pink';
export type Density = 'compact' | 'comfortable' | 'spacious';
export type EstadoOverride = 'auto' | Estado;

export interface Config {
  p_limiar_detectado_auto?: number;  // W (detectado automaticamente pelo firmware)
  t_limite: number;       // segundos
  is_critical: boolean;
  schedule: boolean[];    // length 24, indice = hora (0-23)
  w_i: number;            // peso de criticidade ILP (roteador=1000, TV=10, lampada=1)
  pi_historico: number[]; // [24] probabilidade historica de uso por hora [0,1]
}

export interface Tomada {
  id: string;             // UUID
  residencia_id: string;  // UUID
  nome: string;
  localizacao: string | null;
  icone: Icone;
  config: Config;
  config_version: number;
  updated_at: string;     // ISO
  cstd: number;           // W (consumo standby calibrado)
  con: number;            // W (consumo on calibrado)
  rele_aberto: boolean;
  estado: Estado;
  p_medida_atual: number; // W (leitura atual publicada pelo ESP32)
}

export interface Medicao {
  id: number;
  tomada_id: string;      // UUID
  timestamp: number;      // unix epoch (segundos), bigint no Postgres
  p_medida: number;       // W
  estado: Estado;
  rele_aberto: boolean;
  created_at: string;     // ISO
}

export interface Evento {
  id: number;
  tomada_id: string;
  timestamp: number;      // unix epoch
  tipo: 'chaveamento' | 'inviabilidade' | 'cold_start' | 'config' | 'sync_error';
  severidade: 'info' | 'warn' | 'low';
  titulo: string;
  detalhes: string | null;
  created_at: string;
}

export interface Residencia {
  id: string;
  nome: string;
  p_max: number;         // capacidade do circuito [W] (schema v2)
  created_at: string;
}

// Calculado em runtime a partir das medicoes mais recentes
export interface EstadoAtual {
  estado: Estado;
  p_atual: number;
  tempo_no_estado: number;        // minutos
  ultima_atualizacao_seg: number;
}

// Metricas calculadas em runtime
export interface Metricas {
  consumida: number;        // Wh
  consumidaDelta: number;
  economizada: number;      // Wh
  economizadaDelta: number;
  chaveamentos: number;
  chaveamentosDelta: number;
  tempoOff: string;         // "Xh YYmin"
  tempoOffPct: number;
}

// Estado global da app
export interface AppState {
  residencia: Residencia | null;
  tomadas: Tomada[];
  medicoesPorTomada: Record<string, Medicao[]>;
  eventosPorTomada: Record<string, Evento[]>;
  selectedId: string | null;
  showAll: boolean;

  // Tweaks (persistidos em localStorage)
  scheduleStyle: ScheduleStyle;
  theme: Theme;
  accent: Accent;
  density: Density;
  estadoOverride: EstadoOverride;

  loading: boolean;
  error: string | null;
  toast: string | null;
}
