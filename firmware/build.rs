use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    embuild::espidf::sysenv::output();

    // BUILD_ID unico por compilacao: o firmware compara este valor com o gravado no NVS e,
    // num build novo, limpa Wi-Fi + identidade e abre o portal AP (reconfig de dev sem
    // botao). `cargo:rerun-if-changed` apontando p/ um caminho inexistente forca este
    // script a rodar em TODO build, mantendo o BUILD_ID sempre fresco.
    let build_id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    println!("cargo:rustc-env=BUILD_ID={build_id}");
    println!("cargo:rerun-if-changed=__forca_rerun_build_id__");
}
