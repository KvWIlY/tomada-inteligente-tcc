//! Lógica **pura** de sincronização com a nuvem (host-testável, sem esp-idf).
//!
//! Estas funções não fazem I/O: montam corpos JSON, selecionam registros pendentes e
//! decidem renovação de JWT / adoção de config. O firmware
//! (`firmware/src/comunicacao/backend.rs`) as chama e cuida do transporte HTTP.
//!
//! Contrato vigente: `specs/004-backend-actix-mysql/contracts/backend-api.md`
//! (`POST /telemetria`). Originalmente escrito para a feature 002 contra PostgREST; a
//! feature 004 trocou a camada de nuvem pelo backend próprio e a lógica **não mudou** —
//! o backend ignora o `tomada_id` que `montar_corpo_lote` inclui em cada registro e usa
//! o claim do JWT, e a dedup deixou de ser `on_conflict` para ser
//! `INSERT ... ON DUPLICATE KEY UPDATE` sobre o mesmo `UNIQUE (tomada_id, timestamp)`.
//!
//! Floats são formatados **manualmente** (`{:.2}`) para casar com o caminho do
//! firmware, que evita `serde_json` em `f32`/`[2 x float]` por causa do bug LLVM Xtensa.

use crate::solver::tipos::Estado;

/// Registro sincronizável (espelha `firmware::persistencia::medicao::Medicao`).
///
/// Identidade natural = `(tomada_id, timestamp)` — casa com o `unique (tomada_id,
/// timestamp)` da tabela `medicoes` (schema da 001), base da idempotência de reenvio.
/// `confirmado` é estado **local** (não vai no corpo): `true` só após 2xx da nuvem.
#[derive(Debug, Clone, PartialEq)]
pub struct SyncRecord {
    pub tomada_id: String,
    pub timestamp: i64,
    pub p_medida: f64,
    /// Fator de potencia da mesma janela, em [0, 1]. Vai no corpo do POST.
    pub fator_potencia: f64,
    pub estado: Estado,
    pub rele_aberto: bool,
    pub confirmado: bool,
}

/// Decide se o JWT deve ser renovado: `true` quando faltam `margem_s` segundos (ou
/// menos) para `jwt_exp`, ou se já expirou. Função pura — `agora` e `jwt_exp` em epoch.
///
/// Contrato (research §2): renovação **proativa** por margem + reativa no `401`.
pub fn deve_renovar(jwt_exp: i64, agora: i64, margem_s: i64) -> bool {
    agora >= jwt_exp - margem_s
}

/// Decide se a config remota deve ser adotada: somente quando a versão remota é
/// **estritamente maior** que a local (FR-015 — sem regressão em versões iguais/menores).
pub fn deve_adotar_config(versao_remota: i64, versao_local: i64) -> bool {
    versao_remota > versao_local
}

/// Seleciona os índices dos registros **pendentes** (`confirmado == false`), ordenados
/// por `timestamp` crescente, até no máximo `max` (FR-010 — montagem de lote).
///
/// Retorna índices na fatia original (preserva o vínculo com o buffer do chamador).
/// `max == 0` ⇒ vazio.
pub fn selecionar_pendentes(registros: &[SyncRecord], max: usize) -> Vec<usize> {
    let mut pendentes: Vec<usize> = registros
        .iter()
        .enumerate()
        .filter(|(_, r)| !r.confirmado)
        .map(|(i, _)| i)
        .collect();
    // Ordena por timestamp; desempata por índice para resultado determinístico.
    pendentes.sort_by_key(|&i| (registros[i].timestamp, i));
    pendentes.truncate(max);
    pendentes
}

/// Monta o corpo JSON (array) para `POST /rest/v1/medicoes` a partir dos registros
/// indicados por `indices`. Inclui só as colunas que o role `device` escreve em
/// `medicoes`; **não** inclui `confirmado` (estado local).
///
/// `p_medida` é formatado com 2 casas (`{:.2}`) — mesmo formato do firmware (bug Xtensa).
/// `fator_potencia` usa 3 casas: o valor vive em [0, 1] e 2 casas jogariam fora a
/// diferença entre uma fonte chaveada ruim (0,45) e uma decente (0,55).
/// Um `indices` vazio produz `"[]"`.
pub fn montar_corpo_lote(registros: &[SyncRecord], indices: &[usize]) -> String {
    let mut s = String::from("[");
    for (n, &i) in indices.iter().enumerate() {
        let r = &registros[i];
        if n > 0 {
            s.push(',');
        }
        s.push_str(&format!(
            r#"{{"tomada_id":"{}","timestamp":{},"p_medida":{:.2},"fator_potencia":{:.3},"estado":"{}","rele_aberto":{}}}"#,
            escape_json(&r.tomada_id),
            r.timestamp,
            r.p_medida,
            r.fator_potencia,
            r.estado.as_str(),
            r.rele_aberto,
        ));
    }
    s.push(']');
    s
}

/// Escapa minimamente uma string para uso DENTRO de aspas em JSON (`\` e `"`).
/// Igual ao helper do firmware, replicado aqui para manter as funções puras.
pub fn escape_json(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
