//! Gera `$OUT_DIR/fontes.rs` com um `include_str!` para cada fonte do firmware que o
//! simulador compila. O `firmware.rs` faz o SHA-256 disso: duas rodadas com hashes
//! diferentes nao rodaram o mesmo firmware (research D9).

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let src = manifest.join("..").join("firmware").join("src");
    let solver = src.join("solver");
    println!("cargo:rerun-if-changed={}", solver.display());

    let mut arquivos: Vec<(String, PathBuf)> = Vec::new();
    for entrada in fs::read_dir(&solver).expect("firmware/src/solver") {
        let caminho = entrada.expect("entrada de firmware/src/solver").path();
        if caminho.extension().is_some_and(|x| x == "rs") {
            let nome = caminho.file_name().expect("nome").to_string_lossy().into_owned();
            arquivos.push((format!("solver/{nome}"), caminho));
        }
    }
    for rel in ["sensores/potencia.rs", "persistencia/config.rs", "decisao/ciclo.rs"] {
        arquivos.push((rel.to_string(), src.join(rel)));
    }
    arquivos.sort_by(|a, b| a.0.cmp(&b.0));

    let mut saida = String::from("pub const FONTES: &[(&str, &str)] = &[\n");
    for (nome, caminho) in &arquivos {
        println!("cargo:rerun-if-changed={}", caminho.display());
        let absoluto = caminho.display().to_string().replace('\\', "/");
        writeln!(saida, "    ({nome:?}, include_str!({absoluto:?})),").expect("escrita em String");
    }
    saida.push_str("];\n");

    let destino = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("fontes.rs");
    fs::write(destino, saida).expect("gravando fontes.rs");
}
