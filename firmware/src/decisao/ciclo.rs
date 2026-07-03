//! Ciclo de decisao: monta o `SolverInput` (proprio no + peers ESP-NOW vivos), resolve o
//! MILP (`tomada_solver::resolver`) e decide o estado **da propria tomada**, aplicando o
//! comando manual do dashboard SUBORDINADO ao `P_max` (FR-016). Gera eventos (Principio
//! VII): chaveamento, penalidades (R3/R5) e fantasma eliminado (R4).
//!
//! Mantem estado entre ciclos (calibracao `cstd`/`con`, `ultimo_uso_ativo_ts`,
//! `p_limiar` autodetectado, estado vigente do rele). O I/O (atuar, persistir, sincronizar)
//! fica em `main`/`atuador`/`supabase` — aqui so a logica de decisao.

use tomada_solver::{resolver, Estado, Pesos, SolverInput, TomadaInput};

use crate::persistencia::config::Tomada;
use crate::sensores::potencia::{deriva_f_i, deriva_u_i, Leitura};

/// Limiar de uso padrao (W) ate a autodetecao convergir.
const P_LIMIAR_DEFAULT: f32 = 15.0;
/// Fator do EMA de calibracao (suaviza cstd/con).
const EMA: f32 = 0.1;
/// Consumo minimo (W) p/ considerar a tomada "energizada e consumindo" (calibra cstd).
const P_MIN_STANDBY: f32 = 0.5;

/// Evento gerado pelo ciclo (encaminhado por `main` a `supabase::registrar_evento`).
pub struct EventoCiclo {
    pub tipo: &'static str,
    pub severidade: &'static str,
    pub titulo: String,
    pub detalhes: String,
}

/// Resultado de um ciclo de decisao.
pub struct Decisao {
    pub estado: Estado,
    pub rele_aberto: bool,
    pub p_medida: f32,
    pub cstd: f32,
    pub con: f32,
    pub ultimo_uso_ativo_ts: i64,
    /// `U_i` vigente (uso ativo) — usado p/ montar o heartbeat ESP-NOW.
    pub u_i: bool,
    /// `F_i` vigente (fantasma) — usado p/ montar o heartbeat ESP-NOW.
    pub f_i: bool,
    pub eventos: Vec<EventoCiclo>,
}

/// Estado persistente do ciclo de decisao (vive na main task entre iteracoes).
pub struct Ciclo {
    estado_atual: Estado,
    cstd: f32,
    con: f32,
    ultimo_uso_ativo_ts: i64,
    p_limiar: f32,
}

impl Ciclo {
    /// Inicializa a partir da config local (semente de calibracao/limiar).
    pub fn novo(config: &Tomada, cstd: f32, con: f32, agora: i64) -> Self {
        Ciclo {
            estado_atual: Estado::Off,
            cstd,
            con,
            ultimo_uso_ativo_ts: agora,
            p_limiar: config.p_limiar_detectado_auto.unwrap_or(P_LIMIAR_DEFAULT),
        }
    }

    /// Executa um ciclo de decisao.
    ///
    /// - `leitura`: potencia/tensao/corrente medidas agora.
    /// - `peers_vivos`: `(MAC, TomadaInput)` de cada peer com heartbeat valido (ESP-NOW).
    /// - `carga_fixa`: consumo (W) dos peers AUSENTES (degradacao segura) — subtraido do `P_max`.
    /// - `proprio_mac`: MAC deste no — chave de ordenacao canonica e de auto-localizacao.
    /// - `rele_manual`: comando do dashboard (`rele_aberto`): Some(true)=cortar, Some(false)=manter ligado.
    /// - `hora`: hora do dia (0..23) p/ indexar `Pi`/`schedule`.
    /// - `agora`: epoch atual.
    pub fn decidir(
        &mut self,
        config: &Tomada,
        leitura: Leitura,
        peers_vivos: &[([u8; 6], TomadaInput)],
        carga_fixa: f32,
        proprio_mac: [u8; 6],
        rele_manual: Option<bool>,
        hora: u8,
        agora: i64,
    ) -> Decisao {
        let mut eventos = Vec::new();
        let p_atual = leitura.p;
        let h = (hora % 24) as usize;

        // Limiar de uso: prioriza override da config; senao mantem o autodetectado.
        if let Some(p) = config.p_limiar_detectado_auto {
            self.p_limiar = p;
        }

        // U_i / atualizacao de ultimo uso ativo.
        let u_i = deriva_u_i(p_atual, self.p_limiar);
        if u_i {
            self.ultimo_uso_ativo_ts = agora;
        }
        let ociosa_ha_s = agora - self.ultimo_uso_ativo_ts;

        // F_i: energizado + ocioso por > t_limite.
        let rele_fechado = self.estado_atual != Estado::Off;
        let f_i = deriva_f_i(rele_fechado, p_atual, self.p_limiar, ociosa_ha_s, config.t_limite);

        // Calibracao EMA de cstd/con a partir da medicao real.
        self.calibrar(p_atual, u_i, rele_fechado);

        // TomadaInput do proprio no.
        let propria = TomadaInput {
            c_std: self.cstd as f64,
            c_on: self.con as f64,
            u_i,
            f_i,
            pi: config.pi_historico[h] as f64,
            w_i: config.w_efetivo() as f64,
            a_it: config.schedule[h],
        };

        // Ordem CANONICA por MAC (self + peers): garante entrada IDENTICA em todos os nos
        // (espnow-protocol.md item 4), pre-condicao para o solver determinístico (desempate
        // por indice) convergir na MESMA alocacao global. O proprio no se localiza pelo MAC
        // em vez de assumir um indice fixo.
        let mut com_mac: Vec<([u8; 6], TomadaInput)> = Vec::with_capacity(1 + peers_vivos.len());
        com_mac.push((proprio_mac, propria));
        com_mac.extend_from_slice(peers_vivos);
        com_mac.sort_by(|a, b| a.0.cmp(&b.0));
        let idx_proprio = com_mac
            .iter()
            .position(|(mac, _)| *mac == proprio_mac)
            .unwrap_or(0);
        let tomadas: Vec<TomadaInput> = com_mac.into_iter().map(|(_, t)| t).collect();

        let entrada = SolverInput {
            p_max: config.p_max as f64,
            hora: hora % 24,
            pesos: Pesos::default(),
            carga_fixa: carga_fixa as f64,
            tomadas,
        };

        let saida = resolver(&entrada);
        let mut estado = saida.estados.get(idx_proprio).copied().unwrap_or(Estado::Off);

        // Penalidades do PROPRIO no -> eventos observaveis (R3/R5).
        for p in saida.penalidades.iter().filter(|p| p.tomada_idx == idx_proprio) {
            let (tipo, titulo) = match p.tipo {
                tomada_solver::TipoPenalidade::Uso => ("penalidade_uso", "OFF durante uso ativo (R3)"),
                tomada_solver::TipoPenalidade::Agendamento => ("penalidade_age", "ON fora do horario (R5)"),
            };
            eventos.push(EventoCiclo {
                tipo,
                severidade: "warn",
                titulo: titulo.to_string(),
                detalhes: format!("folga={:.2} Z={:.2}", p.valor, saida.z),
            });
        }

        // Comando manual do dashboard SUBORDINADO ao P_max (FR-016).
        estado = self.aplicar_comando_manual(estado, rele_manual, &entrada, &saida, &mut eventos);

        // Fantasma eliminado (R4): estava fechado, F_i ativo, decidimos OFF.
        if f_i && estado == Estado::Off && rele_fechado {
            eventos.push(EventoCiclo {
                tipo: "fantasma_eliminado",
                severidade: "info",
                titulo: "Consumo fantasma cortado (R4)".to_string(),
                detalhes: format!("p={p_atual:.2} limiar={:.2} ociosa_s={ociosa_ha_s}", self.p_limiar),
            });
        }

        // Chaveamento: mudanca de estado.
        if estado != self.estado_atual {
            eventos.push(EventoCiclo {
                tipo: "chaveamento",
                severidade: "info",
                titulo: format!("{} -> {}", self.estado_atual.as_str(), estado.as_str()),
                detalhes: format!("p={p_atual:.2} hora={hora} Z={:.2}", saida.z),
            });
            self.estado_atual = estado;
        }

        Decisao {
            estado,
            rele_aberto: estado == Estado::Off,
            p_medida: p_atual,
            cstd: self.cstd,
            con: self.con,
            ultimo_uso_ativo_ts: self.ultimo_uso_ativo_ts,
            u_i,
            f_i,
            eventos,
        }
    }

    /// Calibracao EMA: em uso ativo atualiza `con`; energizado e ocioso atualiza `cstd`.
    fn calibrar(&mut self, p_atual: f32, u_i: bool, rele_fechado: bool) {
        if u_i {
            self.con = if self.con <= 0.0 { p_atual } else { self.con * (1.0 - EMA) + p_atual * EMA };
        } else if rele_fechado && p_atual > P_MIN_STANDBY {
            self.cstd = if self.cstd <= 0.0 { p_atual } else { self.cstd * (1.0 - EMA) + p_atual * EMA };
        }
        // Mantem invariante cstd <= con (data-model §2).
        if self.cstd > self.con && self.con > 0.0 {
            self.cstd = self.con;
        }
    }

    /// Aplica o `rele_aberto` do dashboard respeitando o teto eletrico:
    ///   - Some(true) (cortar): sempre honrado (desligar nunca viola P_max).
    ///   - Some(false) (manter ligado): so honrado se couber no P_max disponivel; senao o
    ///     P_max vence (estado permanece OFF) e registra penalidade.
    fn aplicar_comando_manual(
        &self,
        estado_solver: Estado,
        rele_manual: Option<bool>,
        entrada: &SolverInput,
        saida: &tomada_solver::SolverOutput,
        eventos: &mut Vec<EventoCiclo>,
    ) -> Estado {
        match rele_manual {
            Some(true) => Estado::Off, // usuario cortou; sempre seguro
            Some(false) if estado_solver == Estado::Off => {
                // Usuario quer ligado, mas o solver decidiu OFF. So liga (STD) se couber.
                let consumo_total = saida.consumo(entrada);
                let nosso = match estado_solver {
                    Estado::Off => 0.0,
                    Estado::Std => self.cstd as f64,
                    Estado::On => self.con as f64,
                };
                let outros = consumo_total - nosso;
                if outros + self.cstd as f64 <= entrada.p_max - entrada.carga_fixa {
                    Estado::Std
                } else {
                    eventos.push(EventoCiclo {
                        tipo: "penalidade_uso",
                        severidade: "warn",
                        titulo: "Comando manual negado pelo P_max".to_string(),
                        detalhes: format!(
                            "rele_aberto=false negado: outros={outros:.2} cstd={:.2} p_max={:.2}",
                            self.cstd, entrada.p_max
                        ),
                    });
                    Estado::Off
                }
            }
            _ => estado_solver, // sem comando, ou comando coerente com o solver
        }
    }
}
