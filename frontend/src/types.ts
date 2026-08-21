// =============================================
// Tipos do dashboard
// =============================================
//
// Refletem o SCHEMA REAL do backend/MySQL (UUIDs em CHAR(36) + timestamp BIGINT).

export type Estado = 'ON' | 'STD' | 'OFF';
export type Icone = 'printer' | 'tv' | 'fridge' | 'charger' | 'plug';

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
  con: number;            // W (consumo on calibrado / nominal)
  /** Comando manual, TRI-ESTADO (migration 0002 do backend):
   *  `null`  = automatico — o MILP decide (padrao de uma tomada nova);
   *  `true`  = cortar — trava do dono ate ele desfazer;
   *  `false` = "usar agora" — o ESP32 religa por 30 min e volta sozinho ao automatico.
   *  "Sempre ligada" (geladeira) NAO e isto: e `config.is_critical`. */
  rele_aberto: boolean | null;
  estado: Estado;
  p_medida_atual: number; // W (leitura atual publicada pelo ESP32)
  mac?: string;           // identidade do ESP32 (se disponivel no schema)
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
  p_max: number;         // capacidade do circuito [W]
  created_at: string;
}

/** Agregados da residência calculados no servidor (evita recalcular no cliente). */
export interface VisaoResidencia {
  consumo_total_atual: number;
  /** `null` = nenhuma tomada com standby calibrado; a UI mostra "—". */
  efficiency_score: number | null;
}

import type { ToastData, ModalData } from './views/toast';

// Estado global da app
export interface AppState {
  phase: 'checking' | 'login' | 'app'; // gate de auth (checking=verificando sessão)
  userEmail: string | null;
  /** `VITE_BACKEND_URL` ausente: o dashboard não tem a quem perguntar e mostra
   *  uma tela de configuração em vez de dados. */
  semBackend: boolean;
  semResidencia: boolean;      // logado mas sem residência própria

  residencia: Residencia | null;
  /** Agregados calculados pelo BACKEND (`GET /residencias/{id}`) — fonte única do
   *  efficiency score. `null` enquanto não carregou. */
  visao: VisaoResidencia | null;
  tomadas: Tomada[];
  medicoesPorTomada: Record<string, Medicao[]>;
  eventosPorTomada: Record<string, Evento[]>;
  selectedId: string | null;   // null = visão geral
  navOpen: boolean;            // sidebar mobile

  loading: boolean;
  error: string | null;
  toast: ToastData | null;
  modal: ModalData | null;
}
