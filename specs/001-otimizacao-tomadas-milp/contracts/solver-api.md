# Contract — API do crate `tomada-solver`

Feature: 001-otimizacao-tomadas-milp | Princípios I, II, VII

Crate puro (host-testável) que implementa os tópicos 1–4 do `review_modelo.pdf` **sem
alterações**. Sem I/O, sem dependência de hardware: entra `SolverInput`, sai
`SolverOutput`. Determinístico ⇒ reproduzível (Princípio VII).

## Tipos

```rust
pub enum Estado { Off = 0, Std = 1, On = 2 }

pub struct Pesos { pub alpha: f64, pub beta: f64, pub gamma: f64 } // β,γ ≫ 1

pub struct TomadaInput {
    pub c_std: f64,   // C_{i,STD} (C_off = 0 implícito)
    pub c_on:  f64,   // C_{i,ON}
    pub u_i:   bool,  // uso ativo  (R3)
    pub f_i:   bool,  // fantasma   (R4)
    pub pi:    f64,   // Π_{i,t} da hora corrente (0..1)
    pub w_i:   f64,   // criticidade (M se crítico)
    pub a_it:  bool,  // schedule permitido na hora (R5)
}

pub struct SolverInput {
    pub p_max:   f64,            // R2
    pub hora:    u8,             // 0..23 (já usado para indexar pi/a_it no firmware)
    pub pesos:   Pesos,
    pub tomadas: Vec<TomadaInput>,
}

pub struct Penalidade { pub tomada_idx: usize, pub tipo: TipoPenalidade, pub valor: f64 }
pub enum TipoPenalidade { Uso, Agendamento } // y_uso (R3), y_age (R5)

pub struct SolverOutput {
    pub estados: Vec<Estado>,      // x_{i,s} (um por tomada)
    pub z: f64,                    // valor da objetivo
    pub penalidades: Vec<Penalidade>,
    pub factivel: bool,            // sempre true (folgas); exposto p/ asserção
}

pub fn resolver(entrada: &SolverInput) -> SolverOutput;
```

## Função objetivo (tópico 4)

```
min Z = Σ_i Σ_s C_{i,s}·x_{i,s}              (consumo direto)
      + Σ_i α·Π_{i,t}·W_i·x_{i,OFF}          (conforto preditivo)
      + Σ_i (β·y_uso_i + γ·y_age_i)          (penalidades de folga)
```

## Restrições (invariantes verificáveis na saída)

| Regra | Tipo | Invariante na `SolverOutput` |
|-------|------|------------------------------|
| R1 unicidade | hard | exatamente um `Estado` por tomada |
| R2 capacidade | hard | `Σ_i C_{i,estado_i} ≤ p_max` |
| R3 uso ativo | soft | se `u_i` e `estado=OFF` ⇒ `Penalidade::Uso` registrada |
| R4 fantasma | hard | se `f_i` ⇒ `estado ≠ STD` |
| R5 agendamento | soft | se `!a_it` e `estado=ON` ⇒ `Penalidade::Agendamento` |

## Garantias

- **Sempre factível** (`factivel == true`) por construção das folgas (Princípio I).
- **Integralidade**: `estados` sempre inteiros via branch-and-bound (research §1).
- **Determinístico**: mesma entrada ⇒ mesma saída (desempate estável por índice).

## Casos de teste obrigatórios (`tomada-solver/tests/`)

1. **Factibilidade**: soma em ON excede `p_max` ⇒ retorna factível respeitando R2.
2. **R2 prioriza menor `Π`**: desliga primeiro a tomada de menor probabilidade de uso.
3. **R4**: `f_i=true` ⇒ nunca `STD` (vai a OFF se ocioso).
4. **R3**: `u_i=true` forçado a OFF por `p_max` ⇒ paga `y_uso` (penalidade registrada).
5. **R5**: `a_it=false` mas ativação necessária ⇒ paga `y_age`.
6. **Crítico**: `w_i = M` ⇒ tomada crítica preservada mesmo sob pressão de `p_max`.
7. **Cross-check**: para `N ≤ 6`, B&B == enumeração exaustiva (mesmo `Z`).
