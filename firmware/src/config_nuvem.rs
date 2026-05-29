//! Configuracoes lidas do .env em TEMPO DE COMPILACAO.
//!
//! O `dotenv!()` da crate `dotenvy_macro` e uma proc-macro: durante o `cargo build`
//! ela le o arquivo `.env` na raiz do crate e substitui a chamada por uma `&'static str`
//! literal no binario. O `.bin` final e self-contained -- o `.env` nao precisa estar
//! no chip nem em runtime.
//!
//! ATENCAO: as strings ficam HARDCODED no firmware. Qualquer pessoa com acesso ao
//! `.bin` pode extrair com `strings`. Aceitavel para TCC.
//!
//! Esperado em `.env` na raiz deste crate (y/firmware/.env):
//!   WIFI_SSID=...
//!   WIFI_PASSWORD=...
//!   PROJECT_URL=https://xxx.supabase.co
//!   PUBLIC_KEY=eyJhbGciOi...

use dotenvy_macro::dotenv;

// --- Wi-Fi ---
pub const WIFI_SSID:     &str = dotenv!("WIFI_SSID");
pub const WIFI_PASSWORD: &str = dotenv!("WIFI_PASSWORD");

// --- Supabase ---
pub const SUPABASE_URL:      &str = dotenv!("PROJECT_URL");
pub const SUPABASE_ANON_KEY: &str = dotenv!("PUBLIC_KEY");
