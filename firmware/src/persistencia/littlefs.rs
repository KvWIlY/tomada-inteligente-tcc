use anyhow::{anyhow, Result};
use esp_idf_svc::sys;
use std::ffi::CString;

const BASE_PATH: &str = "/lfs";
const PARTITION_LABEL: &str = "storage";

pub fn mount() -> Result<()> {
    let base = CString::new(BASE_PATH)?;
    let label = CString::new(PARTITION_LABEL)?;

    let mut conf: sys::esp_vfs_littlefs_conf_t = unsafe { core::mem::zeroed() };
    conf.base_path = base.as_ptr();
    conf.partition_label = label.as_ptr();
    conf.set_format_if_mount_failed(1);
    conf.set_dont_mount(0);
    conf.set_grow_on_mount(1);

    let err = unsafe { sys::esp_vfs_littlefs_register(&conf) };
    if err != sys::ESP_OK as i32 {
        return Err(anyhow!("esp_vfs_littlefs_register falhou (cod {err})"));
    }

    let (total, used) = espaco()?;
    log::info!("LittleFS montado em {BASE_PATH} -- {used}/{total} bytes em uso");
    Ok(())
}

pub fn base() -> &'static str {
    BASE_PATH
}

/// `(total, usado)` em bytes da particao LittleFS. E a UNICA fonte de espaco livre do
/// dispositivo - a purga por espaco (`persistencia::retencao`) depende dela, entao
/// precisa ser consultavel a qualquer momento, nao so no `mount()`.
pub fn espaco() -> Result<(usize, usize)> {
    let label = CString::new(PARTITION_LABEL)?;
    let (mut total, mut used) = (0usize, 0usize);
    let err = unsafe { sys::esp_littlefs_info(label.as_ptr(), &mut total, &mut used) };
    if err != sys::ESP_OK as i32 {
        return Err(anyhow!("esp_littlefs_info falhou (cod {err})"));
    }
    Ok((total, used))
}
