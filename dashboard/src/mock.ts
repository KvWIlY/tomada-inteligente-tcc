// =============================================
// Gerador de dados mockados (porto fiel do data.jsx)
// =============================================
//
// Usado quando VITE_SUPABASE_URL nao esta definido OU pra testes locais.
// Quando o Supabase esta configurado, supabase.ts ignora isso e usa dados reais.
//
// As tomadas mock tem UUIDs fixos pra serem deterministicas entre reloads.

import type { Evento, Medicao, Residencia, Tomada } from './types';

const RESIDENCIA_MOCK: Residencia = {
  id: '00000000-0000-0000-0000-000000000001',
  nome: 'Apto 304 — Bloco B (mock)',
  created_at: new Date().toISOString(),
};

interface TomadaDef {
  id: string;
  nome: string;
  localizacao: string;
  icone: Tomada['icone'];
  config_version: number;
  config: Tomada['config'];
  perfil: 'esporadico' | 'noturno' | 'critico' | 'carga';
  cstd: number;
  con: number;
}

const TOMADAS_DEF: TomadaDef[] = [
  {
    id: '00000000-0000-0000-0000-000000000101',
    nome: 'Impressora HP', localizacao: 'Escritorio', icone: 'printer',
    config_version: 7,
    config: {
      p_limiar_detectado_auto: 8, t_limite: 1800, is_critical: false,
      schedule: [false,false,false,false,false,false,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,false,false,false],
      w_i: 10, p_max: 2540,
      pi_historico: [0.0,0.0,0.0,0.0,0.0,0.0,0.1,0.3,0.5,0.6,0.7,0.5,0.4,0.3,0.5,0.6,0.7,0.6,0.4,0.2,0.1,0.0,0.0,0.0],
    },
    perfil: 'esporadico', cstd: 3.2, con: 142.8,
  },
  {
    id: '00000000-0000-0000-0000-000000000102',
    nome: 'TV Sala', localizacao: 'Sala de estar', icone: 'tv',
    config_version: 3,
    config: {
      p_limiar_detectado_auto: 12, t_limite: 3600, is_critical: false,
      schedule: [false,false,false,false,false,false,false,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,false],
      w_i: 10, p_max: 2540,
      pi_historico: [0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.1,0.1,0.1,0.1,0.1,0.1,0.1,0.2,0.3,0.5,0.7,0.9,0.95,0.9,0.8,0.6,0.2],
    },
    perfil: 'noturno', cstd: 6.4, con: 95,
  },
  {
    id: '00000000-0000-0000-0000-000000000103',
    nome: 'Geladeira', localizacao: 'Cozinha', icone: 'fridge',
    config_version: 2,
    config: {
      p_limiar_detectado_auto: 30, t_limite: 1800, is_critical: true,
      schedule: Array(24).fill(true) as boolean[],
      w_i: 1000, p_max: 2540,
      pi_historico: Array(24).fill(0.9) as number[],
    },
    perfil: 'critico', cstd: 0, con: 95,
  },
  {
    id: '00000000-0000-0000-0000-000000000104',
    nome: 'Carregadores', localizacao: 'Quarto', icone: 'charger',
    config_version: 5,
    config: {
      p_limiar_detectado_auto: 4, t_limite: 900, is_critical: false,
      schedule: [false,false,false,false,false,false,true,true,false,false,false,false,false,false,false,false,false,false,true,true,true,true,true,false],
      w_i: 1, p_max: 2540,
      pi_historico: [0.0,0.0,0.0,0.0,0.0,0.0,0.3,0.5,0.1,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.4,0.6,0.7,0.8,0.5,0.1],
    },
    perfil: 'carga', cstd: 2.1, con: 18,
  },
];

// Gera 1440 medicoes (24h x 60min) para uma tomada conforme seu perfil
function gerarMedicoes(def: TomadaDef): Medicao[] {
  const agoraSec = Math.floor(Date.now() / 1000);
  const minutos = 24 * 60;
  const out: Medicao[] = [];
  const { perfil, config, cstd } = def;

  let usos: { startMin: number; durMin: number; peak: number }[] = [];
  if (perfil === 'esporadico') {
    usos = [
      { startMin: 1440 - 20,  durMin: 6, peak: 145 },
      { startMin: 1440 - 180, durMin: 4, peak: 132 },
      { startMin: 1440 - 420, durMin: 8, peak: 168 },
      { startMin: 1440 - 720, durMin: 5, peak: 128 },
      { startMin: 1440 - 980, durMin: 3, peak: 95 },
    ];
  } else if (perfil === 'noturno') {
    usos = [
      { startMin: 1440 - 60,  durMin: 180, peak: 95 },
      { startMin: 1440 - 720, durMin: 240, peak: 90 },
    ];
  } else if (perfil === 'carga') {
    usos = [
      { startMin: 1440 - 420,  durMin: 90,  peak: 18 },
      { startMin: 1440 - 1140, durMin: 120, peak: 15 },
    ];
  }
  const isCritico = perfil === 'critico';

  for (let i = 0; i < minutos; i++) {
    const ts = agoraSec - (minutos - i) * 60;
    let p = 0;
    let estado: Medicao['estado'] = 'OFF';
    let releAberto = true;
    const hora = new Date(ts * 1000).getHours();
    const permitido = config.schedule[hora];

    if (isCritico) {
      // Compressor liga ~12min a cada 30min
      const fase = i % 30;
      if (fase < 12) {
        p = 90 + Math.random() * 12;
        estado = 'ON';
      } else {
        p = 2 + Math.random() * 1.2;
        estado = 'STD';
      }
      releAberto = false;
    } else {
      const standbyBase = cstd * (0.85 + Math.random() * 0.3);
      const uso = usos.find((u) => i >= u.startMin && i < u.startMin + u.durMin);
      if (uso) {
        const phase = (i - uso.startMin) / uso.durMin;
        p = uso.peak * (0.7 + 0.3 * Math.sin(phase * Math.PI)) + Math.random() * 6;
        estado = 'ON';
        releAberto = false;
      } else {
        const ultimoUso = usos
          .filter((u) => u.startMin + u.durMin <= i)
          .sort((a, b) => b.startMin - a.startMin)[0];
        const minDesdeUso = ultimoUso ? i - (ultimoUso.startMin + ultimoUso.durMin) : i;
        if (!permitido || minDesdeUso > 30) {
          p = 0; estado = 'OFF'; releAberto = true;
        } else {
          p = standbyBase; estado = 'STD'; releAberto = false;
        }
      }
    }

    out.push({
      id: i,
      tomada_id: def.id,
      timestamp: ts,
      p_medida: Math.max(0, p),
      estado,
      rele_aberto: releAberto,
      created_at: new Date(ts * 1000).toISOString(),
    });
  }
  return out;
}

function gerarEventos(def: TomadaDef, medicoes: Medicao[]): Evento[] {
  const out: Evento[] = [];
  if (medicoes.length === 0) return out;
  let lastEstado = medicoes[0].estado;
  let nextId = 1;

  for (let i = 1; i < medicoes.length; i++) {
    const m = medicoes[i];
    if (m.estado !== lastEstado) {
      let motivo = '';
      if (lastEstado === 'STD' && m.estado === 'OFF') motivo = 'T_ocioso atingiu 30 min, acima do limite';
      else if (lastEstado === 'OFF' && m.estado === 'ON') motivo = `P_atual ${m.p_medida.toFixed(1)} W > P_limiar`;
      else if (lastEstado === 'ON' && m.estado === 'STD') motivo = 'Uso ativo finalizado, aparelho em standby';
      else if (lastEstado === 'STD' && m.estado === 'ON') motivo = `P_atual ${m.p_medida.toFixed(1)} W > P_limiar`;
      else if (lastEstado === 'OFF' && m.estado === 'STD') motivo = 'Horario permitido, restaurando standby';
      else if (lastEstado === 'ON' && m.estado === 'OFF') motivo = 'Fora do horario permitido apos uso';

      out.push({
        id: nextId++,
        tomada_id: def.id,
        timestamp: m.timestamp,
        tipo: 'chaveamento',
        severidade: 'info',
        titulo: `Rele ${m.rele_aberto ? 'aberto' : 'fechado'}: ${lastEstado} → ${m.estado}`,
        detalhes: motivo,
        created_at: new Date(m.timestamp * 1000).toISOString(),
      });
      lastEstado = m.estado;
    }
  }
  return out.sort((a, b) => b.timestamp - a.timestamp);
}

/** Constroi o dataset mock completo: residencia + tomadas + medicoes + eventos */
export function buildMockData() {
  const tomadas: Tomada[] = [];
  const medicoesPorTomada: Record<string, Medicao[]> = {};
  const eventosPorTomada: Record<string, Evento[]> = {};

  for (const def of TOMADAS_DEF) {
    const meds = gerarMedicoes(def);
    const evs  = gerarEventos(def, meds);
    const ult  = meds[meds.length - 1];
    tomadas.push({
      id: def.id,
      residencia_id: RESIDENCIA_MOCK.id,
      nome: def.nome,
      localizacao: def.localizacao,
      icone: def.icone,
      config: def.config,
      config_version: def.config_version,
      updated_at: new Date().toISOString(),
      cstd: def.cstd,
      con: def.con,
      rele_aberto: ult?.rele_aberto ?? false,
      estado: ult?.estado ?? 'OFF',
      p_medida_atual: ult?.p_medida ?? 0,
    });
    medicoesPorTomada[def.id] = meds;
    eventosPorTomada[def.id]  = evs;
  }

  return {
    residencia: RESIDENCIA_MOCK,
    tomadas,
    medicoesPorTomada,
    eventosPorTomada,
  };
}
