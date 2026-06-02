//! Configuracoes de BOOTSTRAP lidas do .env em TEMPO DE COMPILACAO.
//!
//! O `dotenv!()` da crate `dotenvy_macro` e uma proc-macro: durante o `cargo build`
//! ela le o arquivo `.env` na raiz do crate e substitui a chamada por uma `&'static str`
//! literal no binario. O `.bin` final e self-contained -- o `.env` nao precisa estar
//! no chip nem em runtime.
//!
//! SEGURANCA (Principio VIII / feature 002, FR-002): este arquivo expoe SOMENTE a
//! superficie minima de bootstrap -- `SUPABASE_URL` + credenciais de Wi-Fi. NAO ha
//! `SUPABASE_ANON_KEY` nem identificadores por-dispositivo (`TOMADA_ID`/`RESIDENCIA_ID`)
//! em compile-time: a anon key compartilhada foi removida (era uma violacao do Principio
//! VIII) e a identidade/auth do dispositivo vem do cache de credenciais provisionadas
//! (`persistencia::credenciais`: `tomada_id`/`residencia_id`/`device_secret`/`jwt`),
//! nunca do binario. O `.bin` e generico (SC-001/SC-002).
//!
//! Esperado em `.env` na raiz deste crate (y/firmware/.env):
//!   WIFI_SSID=...
//!   WIFI_PASSWORD=...
//!   PROJECT_URL=https://xxx.supabase.co
//!   SUPABASE_PUBLISHABLE_KEY=sb_publishable_...
//!
//! NOTA sobre `SUPABASE_PUBLISHABLE_KEY` (correcao pos-integracao real, contracts/
//! supabase-client.md §1): o gateway do Supabase hospedado EXIGE o header
//! `apikey: <publishable key>` em todo request a `/rest/v1/*` -- senao responde
//! `401 "No API key found"` ANTES da RLS. A publishable key e PUBLICA (`sb_publishable_…`),
//! feita para o cliente, e NAO concede acesso sozinha (RLS default-deny). NAO fere o
//! Principio VIII: o segredo/identidade continua sendo so o JWT por-device. Por ser
//! publica, pode viver em compile-time (diferente da antiga anon key, que dava acesso).

use dotenvy_macro::dotenv;

// --- Wi-Fi (bootstrap) ---
pub const WIFI_SSID:     &str = dotenv!("WIFI_SSID");
pub const WIFI_PASSWORD: &str = dotenv!("WIFI_PASSWORD");

// --- Supabase ---
// URL do projeto (a auth de dados e por JWT por-dispositivo, no cache de credenciais).
pub const SUPABASE_URL: &str = dotenv!("PROJECT_URL");
// Publishable key (PUBLICA) -- header `apikey` obrigatorio do gateway hospedado.
pub const SUPABASE_PUBLISHABLE_KEY: &str = dotenv!("SUPABASE_PUBLISHABLE_KEY");

// --- Provisionamento (bootstrap de 1a vez) ---
// Claim-code de USO UNICO gerado no dashboard. Usado UMA vez no 1o boot p/ obter o
// segredo durável + JWT (gravados em LittleFS); depois fica inerte (pode esvaziar no
// .env). NAO e segredo de dados nem identificador por-device — e por-residencia e
// expira/queima no primeiro uso. Em producao, o ideal e coletá-lo no portal AP.
// Deixe `CLAIM_CODE=` (vazio) se o dispositivo ja estiver provisionado.
pub const CLAIM_CODE: &str = dotenv!("CLAIM_CODE");
