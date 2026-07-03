//! Testes host da funcao pura do fallback de coordenacao pela nuvem (feature 003).
//! Cada regra do contrato (specs/003-.../contracts/cloud-fallback.md) tem um teste.

use tomada_solver::coordenacao::{carga_fixa_nuvem, contar_frescas, RemoteTomada};

const AGORA: i64 = 1_750_000_000;
const JANELA: i64 = 300;
const PROPRIO: &str = "aaaa-1111";

fn irma(id: &str, p: f32, ts: i64) -> RemoteTomada {
    RemoteTomada {
        tomada_id: id.to_string(),
        p_medida_atual: p,
        ultimo_uso_ativo_ts: ts,
    }
}

#[test]
fn vazio_retorna_zero() {
    assert_eq!(carga_fixa_nuvem(PROPRIO, AGORA, JANELA, &[]), 0.0);
    // "Vazio" logico: a GET devolve a propria tomada tambem.
    let so_eu = [irma(PROPRIO, 500.0, AGORA)];
    assert_eq!(carga_fixa_nuvem(PROPRIO, AGORA, JANELA, &so_eu), 0.0);
}

#[test]
fn exclui_propria_tomada() {
    let irmas = [irma(PROPRIO, 900.0, AGORA), irma("bbbb-2222", 100.0, AGORA)];
    assert_eq!(carga_fixa_nuvem(PROPRIO, AGORA, JANELA, &irmas), 100.0);
}

#[test]
fn descarta_irma_obsoleta() {
    let irmas = [irma("bbbb-2222", 800.0, AGORA - JANELA - 1)];
    assert_eq!(carga_fixa_nuvem(PROPRIO, AGORA, JANELA, &irmas), 0.0);
}

#[test]
fn borda_da_janela_inclui() {
    // `agora - ts == janela` => INCLUIDA (criterio e <=).
    let irmas = [irma("bbbb-2222", 150.0, AGORA - JANELA)];
    assert_eq!(carga_fixa_nuvem(PROPRIO, AGORA, JANELA, &irmas), 150.0);
}

#[test]
fn ts_futuro_conta() {
    // Relogio da irma adiantado (skew SNTP): `agora - ts` negativo <= janela.
    // Contar e o comportamento conservador correto — este teste o torna contrato.
    let irmas = [irma("bbbb-2222", 200.0, AGORA + 50)];
    assert_eq!(carga_fixa_nuvem(PROPRIO, AGORA, JANELA, &irmas), 200.0);
}

#[test]
fn clamp_negativo() {
    // p_medida_atual < 0 (sensor/calibracao ruim) => tratado como 0, nunca reduz a soma.
    let irmas = [irma("bbbb-2222", -50.0, AGORA), irma("cccc-3333", 120.0, AGORA)];
    assert_eq!(carga_fixa_nuvem(PROPRIO, AGORA, JANELA, &irmas), 120.0);
}

#[test]
fn soma_irmas_frescas() {
    let irmas = [
        irma(PROPRIO, 999.0, AGORA),                    // propria: fora
        irma("bbbb-2222", 100.5, AGORA - 10),           // fresca: entra
        irma("cccc-3333", 60.0, AGORA - JANELA),        // borda: entra
        irma("dddd-4444", 800.0, AGORA - JANELA - 60),  // obsoleta: fora
    ];
    let carga = carga_fixa_nuvem(PROPRIO, AGORA, JANELA, &irmas);
    assert!((carga - 160.5).abs() < f32::EPSILON * 100.0, "carga={carga}");
    assert_eq!(contar_frescas(PROPRIO, AGORA, JANELA, &irmas), 2);
}
