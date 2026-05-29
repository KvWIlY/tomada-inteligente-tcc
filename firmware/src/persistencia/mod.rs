//! Persistencia local em LittleFS.
//!
//! Infra base: `littlefs::mount()` registra o VFS em `/lfs` e cria a particao
//! se nao existir. Tudo que escrever em `/lfs/...` vai para a flash.
//!
//! Adicione aqui modulos especificos do dominio do novo projeto
//! (ex.: arquivos de medicao, config, fila offline) usando `littlefs::base()`
//! como ponto de montagem.

pub mod littlefs;

use anyhow::Result;

pub fn init() -> Result<()> {
    littlefs::mount()?;
    Ok(())
}
