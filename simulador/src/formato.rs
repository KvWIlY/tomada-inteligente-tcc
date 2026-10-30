//! Formatacao de numeros e duracoes. CSV e JSON sao para maquina (ponto decimal, sem
//! milhar); stdout, stderr e `relatorio.md` sao para gente (virgula decimal, ponto de milhar).

use std::borrow::Cow;

/// `43200` -> `43.200`.
pub fn milhar(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push('.');
        }
        out.push(c);
    }
    out
}

/// `447.912` com 1 casa -> `447,9`.
pub fn virgula(x: f64, casas: usize) -> String {
    format!("{x:.casas$}").replace('.', ",")
}

/// Segundos -> `HH:MM:SS` (horas passam de 99 se precisar).
pub fn hms(segundos: f64) -> String {
    let s = segundos.max(0.0).round() as u64;
    format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

/// Segundos -> `20,0 s`, `11 min 36 s`, `2 h 05 min`, `3 d 4 h`.
pub fn duracao_humana(segundos: f64) -> String {
    let s = segundos.max(0.0);
    if s < 60.0 {
        return format!("{} s", virgula(s, 1));
    }
    let t = s.round() as u64;
    if t < 3600 {
        format!("{} min {} s", t / 60, t % 60)
    } else if t < 86_400 {
        format!("{} h {:02} min", t / 3600, (t / 60) % 60)
    } else {
        format!("{} d {} h", t / 86_400, (t / 3600) % 24)
    }
}

/// Duracao declarada em segundos -> `30 d`, `48 h`, `90 min`, `45 s`.
pub fn duracao_curta(segundos: i64) -> String {
    if segundos > 0 && segundos % 86_400 == 0 {
        format!("{} d", segundos / 86_400)
    } else if segundos > 0 && segundos % 3600 == 0 {
        format!("{} h", segundos / 3600)
    } else if segundos > 0 && segundos % 60 == 0 {
        format!("{} min", segundos / 60)
    } else {
        format!("{segundos} s")
    }
}

/// Numero para CSV: ponto decimal, no maximo `casas` casas, sem zeros inuteis.
pub fn num(x: f64, casas: usize) -> String {
    let s = format!("{x:.casas$}");
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
    if s == "-0" { "0".to_string() } else { s }
}

/// Campo de CSV: aspas duplas quando ha `,`, `"` ou quebra de linha, com as aspas internas
/// duplicadas.
pub fn csv(campo: &str) -> Cow<'_, str> {
    if campo.contains([',', '"', '\n', '\r']) {
        Cow::Owned(format!("\"{}\"", campo.replace('"', "\"\"")))
    } else {
        Cow::Borrowed(campo)
    }
}
