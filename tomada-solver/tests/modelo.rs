//! Testes do modelo MILP (contracts/solver-api.md, casos 1-7).
//! Rodam no host: `cargo test -p tomada-solver`.

use tomada_solver::{resolver, Estado, Pesos, SolverInput, TipoPenalidade, TomadaInput};

/// Tomada padrao: nao-critica, sem uso/fantasma, horario permitido, sem historico.
fn tomada(c_std: f64, c_on: f64) -> TomadaInput {
    TomadaInput {
        c_std,
        c_on,
        u_i: false,
        f_i: false,
        pi: 0.0,
        w_i: 1.0,
        a_it: true,
    }
}

fn entrada(p_max: f64, tomadas: Vec<TomadaInput>) -> SolverInput {
    SolverInput { p_max, hora: 12, pesos: Pesos::default(), carga_fixa: 0.0, tomadas }
}

fn consumo_total(estados: &[Estado], ts: &[TomadaInput]) -> f64 {
    estados
        .iter()
        .zip(ts)
        .map(|(e, t)| match e {
            Estado::Off => 0.0,
            Estado::Std => t.c_std,
            Estado::On => t.c_on,
        })
        .sum()
}

/// Caso 1 — Factibilidade: soma em ON excede p_max => factivel respeitando R2.
#[test]
fn factibilidade_sob_teto() {
    let ts = vec![tomada(5.0, 100.0), tomada(5.0, 100.0), tomada(5.0, 100.0)];
    let inp = entrada(150.0, ts.clone()); // 3x100=300 > 150
    let out = resolver(&inp);

    assert!(out.factivel);
    assert_eq!(out.estados.len(), 3);
    // R1: um estado por tomada (garantido pelo tipo); R2: respeita o teto.
    assert!(consumo_total(&out.estados, &ts) <= 150.0 + 1e-6);
}

/// Caso 2 — R2 prioriza menor Pi; e B&B == enumeracao exaustiva (cross-check).
#[test]
fn r2_prioriza_menor_pi_e_cross_check() {
    // Ambas QUEREM ficar ligadas (conforto alpha*Pi*W > c_std), mas o teto so cabe uma.
    // A de maior conforto fica em STD (energizada); a de menor conforto vai a OFF.
    let mut a = tomada(40.0, 100.0);
    a.pi = 1.0;
    a.w_i = 100.0; // conforto OFF = 100 > c_std 40 -> quer ficar ligada
    let mut b = tomada(40.0, 100.0);
    b.pi = 1.0;
    b.w_i = 50.0; // conforto OFF = 50 > 40 -> tambem quer, mas vale menos
    let ts = vec![a, b];
    let inp = entrada(40.0, ts.clone()); // so cabe uma energizada (40 <= 40)

    let out = resolver(&inp);
    assert!(out.factivel);
    assert!(consumo_total(&out.estados, &ts) <= 40.0 + 1e-6);
    // A de maior conforto (idx 0) preservada (energizada); a menor (idx 1) desligada.
    assert_ne!(out.estados[0], Estado::Off);
    assert_eq!(out.estados[1], Estado::Off);

    // Cross-check com enumeracao exaustiva.
    let (z_enum, _) = enumeracao(&inp);
    assert!((out.z - z_enum).abs() < 1e-4, "B&B z={} enum z={}", out.z, z_enum);
}

/// Caso 3 — R4: F_i => nunca STD.
#[test]
fn r4_fantasma_proibe_std() {
    let mut t = tomada(10.0, 100.0);
    t.f_i = true;
    let ts = vec![t];
    let inp = entrada(1000.0, ts); // teto folgado: a escolha nao e por capacidade
    let out = resolver(&inp);

    assert_ne!(out.estados[0], Estado::Std, "STD proibido sob fantasma (R4)");
    // Sem uso ativo e fantasma detectado: esperado OFF (consome menos que ON).
    assert_eq!(out.estados[0], Estado::Off);
}

/// Caso 4 — R3: uso ativo forcado a OFF por p_max => paga y_uso.
#[test]
fn r3_uso_ativo_paga_penalidade() {
    // Uma tomada em uso ativo, mas o teto e 0 (nada pode ficar ligado).
    let mut t = tomada(10.0, 100.0);
    t.u_i = true;
    let ts = vec![t];
    let inp = entrada(0.0, ts); // forca OFF

    let out = resolver(&inp);
    assert_eq!(out.estados[0], Estado::Off);
    assert!(
        out.penalidades.iter().any(|p| p.tipo == TipoPenalidade::Uso && p.tomada_idx == 0),
        "esperava penalidade de uso (y_uso) ativada"
    );
}

/// Caso 5 — R5: horario proibido mas ON e' a melhor opcao => paga y_age.
#[test]
fn r5_fora_horario_paga_penalidade() {
    // STD proibido (F_i, R4) => "energizado" so pode ser ON. Aparelho critico e sempre
    // usado (conforto OFF altissimo) e fora do horario (a_it=false): o modelo mantem ON
    // pagando a penalidade de agendamento, em vez de cortar um aparelho critico.
    let mut t = tomada(10.0, 100.0);
    t.f_i = true; // R4 proibe STD -> energizado = ON
    t.a_it = false; // horario proibido
    t.pi = 1.0;
    t.w_i = 1.0e6; // conforto OFF = 1e6 (cortar e' caríssimo)
    let ts = vec![t];
    let mut inp = entrada(1000.0, ts); // teto folgado: ligar nao viola R2
    // OFF custa ~1e6 (conforto); ON custa c_on(100) + gamma(1e3). gamma << conforto -> ON.
    inp.pesos = Pesos { alpha: 1.0, beta: 1.0e6, gamma: 1.0e3 };

    let out = resolver(&inp);
    assert_eq!(out.estados[0], Estado::On);
    assert!(
        out.penalidades.iter().any(|p| p.tipo == TipoPenalidade::Agendamento),
        "esperava penalidade de agendamento (y_age) ativada"
    );
}

/// Caso 6 — Criticidade: w_i = M preserva a tomada sob pressao de p_max.
#[test]
fn criticidade_preserva_tomada() {
    const M: f64 = 1.0e6;
    let mut critica = tomada(40.0, 100.0);
    critica.pi = 0.5;
    critica.w_i = M; // critica (roteador/geladeira): conforto OFF = 0.5*M >> c_std
    let mut comum = tomada(40.0, 100.0);
    comum.pi = 0.5;
    comum.w_i = 1.0;
    let ts = vec![critica, comum];
    let inp = entrada(40.0, ts.clone()); // so cabe uma energizada (40 <= 40)

    let out = resolver(&inp);
    assert!(consumo_total(&out.estados, &ts) <= 40.0 + 1e-6);
    // A critica (idx 0) deve ser preservada (energizada); a comum (idx 1) desligada.
    assert_ne!(out.estados[0], Estado::Off, "tomada critica deve ser preservada");
    assert_eq!(out.estados[1], Estado::Off);
}

/// Caso 7 — Cross-check geral B&B == enumeracao (N<=6) em cenario aleatorio fixo.
#[test]
fn cross_check_enumeracao() {
    let ts = vec![
        { let mut t = tomada(3.0, 60.0); t.pi = 0.2; t },
        { let mut t = tomada(2.0, 40.0); t.pi = 0.8; t.u_i = true; t },
        { let mut t = tomada(5.0, 90.0); t.pi = 0.5; t.f_i = true; t },
        { let mut t = tomada(1.0, 25.0); t.pi = 0.1; t.a_it = false; t },
    ];
    let inp = entrada(120.0, ts);
    let out = resolver(&inp);
    let (z_enum, _) = enumeracao(&inp);
    assert!(out.factivel);
    assert!((out.z - z_enum).abs() < 1e-4, "B&B z={} enum z={}", out.z, z_enum);
}

// ---------------------------------------------------------------------------
// Enumeracao exaustiva de referencia (3^N) p/ cross-check do B&B.
// Replica a objetivo e R1-R5 com folgas resolvidas de forma fechada.
// ---------------------------------------------------------------------------
fn enumeracao(inp: &SolverInput) -> (f64, Vec<Estado>) {
    let n = inp.tomadas.len();
    let total = 3usize.pow(n as u32);
    let mut melhor_z = f64::INFINITY;
    let mut melhor: Vec<Estado> = vec![Estado::Off; n];

    for combo in 0..total {
        let mut estados = Vec::with_capacity(n);
        let mut c = combo;
        for _ in 0..n {
            estados.push(Estado::TODOS[c % 3]);
            c /= 3;
        }

        // R4 hard: F_i => nao STD.
        let mut valido = true;
        let mut consumo = 0.0;
        let mut z = 0.0;
        for (i, t) in inp.tomadas.iter().enumerate() {
            let e = estados[i];
            if e == Estado::Std && t.f_i {
                valido = false;
                break;
            }
            let c_is = match e {
                Estado::Off => 0.0,
                Estado::Std => t.c_std,
                Estado::On => t.c_on,
            };
            consumo += c_is;
            z += c_is;
            // 2o termo: conforto preditivo no OFF.
            if e == Estado::Off {
                z += inp.pesos.alpha * t.pi * t.w_i;
            }
            // Folgas (resolvidas no minimo): R3 e R5.
            // R3: x_off <= 1 - u + y_uso => y_uso >= x_off - (1-u).
            let x_off: f64 = if e == Estado::Off { 1.0 } else { 0.0 };
            let u: f64 = if t.u_i { 1.0 } else { 0.0 };
            let y_uso = (x_off - (1.0 - u)).max(0.0);
            // R5: x_on <= a + y_age => y_age >= x_on - a.
            let x_on: f64 = if e == Estado::On { 1.0 } else { 0.0 };
            let a: f64 = if t.a_it { 1.0 } else { 0.0 };
            let y_age = (x_on - a).max(0.0);
            z += inp.pesos.beta * y_uso + inp.pesos.gamma * y_age;
        }
        if !valido {
            continue;
        }
        // R2 hard.
        if consumo > (inp.p_max - inp.carga_fixa).max(0.0) + 1e-9 {
            continue;
        }
        if z < melhor_z {
            melhor_z = z;
            melhor = estados;
        }
    }
    (melhor_z, melhor)
}
