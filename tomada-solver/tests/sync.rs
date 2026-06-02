//! Testes da logica pura de sincronizacao (host) — feature 002-refinamento-supabase.
//! Contrato: specs/002-refinamento-supabase/contracts/supabase-client.md.

use tomada_solver::sync::{
    deve_adotar_config, deve_renovar, montar_corpo_lote, selecionar_pendentes, SyncRecord,
};
use tomada_solver::tipos::Estado;

fn rec(tomada_id: &str, timestamp: i64, p: f64, estado: Estado, confirmado: bool) -> SyncRecord {
    SyncRecord {
        tomada_id: tomada_id.to_string(),
        timestamp,
        p_medida: p,
        estado,
        rele_aberto: estado == Estado::Off,
        confirmado,
    }
}

// T006 — deve_renovar (margem + expiracao)
#[test]
fn t006_renova_dentro_da_margem_e_apos_expirar() {
    let exp = 1_000;
    let margem = 300; // 5 min
    assert!(!deve_renovar(exp, 600, margem)); // longe da margem
    assert!(deve_renovar(exp, 700, margem)); // inicio da margem (exp - margem)
    assert!(deve_renovar(exp, 950, margem)); // dentro da margem
    assert!(deve_renovar(exp, 1_500, margem)); // ja expirado
    assert!(!deve_renovar(exp, 999, 0)); // margem zero: so em/apos exp
    assert!(deve_renovar(exp, 1_000, 0));
}

// T025 — deve_adotar_config (so versao estritamente maior; sem regressao)
#[test]
fn t025_adota_config_so_se_versao_remota_maior() {
    assert!(deve_adotar_config(5, 4));
    assert!(!deve_adotar_config(4, 4));
    assert!(!deve_adotar_config(3, 4));
    assert!(deve_adotar_config(1, 0));
    assert!(!deve_adotar_config(0, 0));
}

// T019 — selecionar_pendentes (so confirmado=false, ordenado por timestamp, <= max)
#[test]
fn t019_seleciona_so_pendentes_ordenado_por_timestamp() {
    let buf = vec![
        rec("a", 30, 10.0, Estado::On, false),  // idx 0
        rec("a", 10, 11.0, Estado::On, true),   // idx 1 (confirmado -> fora)
        rec("a", 20, 12.0, Estado::Std, false), // idx 2
        rec("a", 40, 13.0, Estado::Off, false), // idx 3
    ];
    assert_eq!(selecionar_pendentes(&buf, 10), vec![2, 0, 3]);
}

#[test]
fn t019_respeita_max_e_max_zero() {
    let buf = vec![
        rec("a", 30, 10.0, Estado::On, false),
        rec("a", 20, 12.0, Estado::Std, false),
        rec("a", 40, 13.0, Estado::Off, false),
    ];
    assert_eq!(selecionar_pendentes(&buf, 2), vec![1, 0]);
    assert!(selecionar_pendentes(&buf, 0).is_empty());
}

#[test]
fn t019_tudo_confirmado_retorna_vazio() {
    let buf = vec![
        rec("a", 10, 1.0, Estado::On, true),
        rec("a", 20, 2.0, Estado::On, true),
    ];
    assert!(selecionar_pendentes(&buf, 10).is_empty());
}

// T018 — montar_corpo_lote (array JSON, formato {:.2}, sem 'confirmado', escape)
#[test]
fn t018_corpo_lote_formato_e_colunas() {
    let buf = vec![
        rec("uuid-1", 100, 82.4, Estado::On, false),
        rec("uuid-1", 160, 0.0, Estado::Off, false),
    ];
    let idx = selecionar_pendentes(&buf, 10);
    let body = montar_corpo_lote(&buf, &idx);
    let esperado = concat!(
        "[",
        r#"{"tomada_id":"uuid-1","timestamp":100,"p_medida":82.40,"estado":"ON","rele_aberto":false}"#,
        ",",
        r#"{"tomada_id":"uuid-1","timestamp":160,"p_medida":0.00,"estado":"OFF","rele_aberto":true}"#,
        "]"
    );
    assert_eq!(body, esperado);
    assert!(!body.contains("confirmado"));
}

#[test]
fn t018_corpo_lote_vazio() {
    let buf: Vec<SyncRecord> = vec![];
    assert_eq!(montar_corpo_lote(&buf, &[]), "[]");
}

#[test]
fn t018_corpo_lote_escapa_aspas_no_id() {
    let buf = vec![rec(r#"a"b\c"#, 5, 1.0, Estado::On, false)];
    let body = montar_corpo_lote(&buf, &[0]);
    assert!(body.contains(r#""tomada_id":"a\"b\\c""#), "body={body}");
}

// Idempotencia (FR-013): reenvio do mesmo lote gera corpo identico.
#[test]
fn reenvio_do_mesmo_lote_e_deterministico() {
    let buf = vec![
        rec("t1", 20, 5.0, Estado::On, false),
        rec("t1", 10, 6.0, Estado::Std, false),
    ];
    let a = montar_corpo_lote(&buf, &selecionar_pendentes(&buf, 10));
    let b = montar_corpo_lote(&buf, &selecionar_pendentes(&buf, 10));
    assert_eq!(a, b);
}
