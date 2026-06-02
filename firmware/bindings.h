/* Cabecalhos extras para o esp-idf-sys gerar bindings (alem do conjunto padrao do IDF).
 * Sem `bindings_module` no Cargo.toml, os simbolos entram na raiz de `esp_idf_sys`
 * (acessiveis via `esp_idf_svc::sys::...`). Aqui: a API do componente joltwallet/littlefs
 * (`esp_vfs_littlefs_conf_t`, `esp_vfs_littlefs_register`, `esp_littlefs_info`). */
#include "esp_littlefs.h"
