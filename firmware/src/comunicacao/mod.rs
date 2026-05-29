pub mod sntp;
pub mod wifi;
pub mod wifi_provisioning;

// `supabase.rs.template` esta preservado como referencia do projeto antigo
// (cliente HTTPS para Supabase com auth, GET/POST, fila offline). Para usar:
//   1. Renomeie para `supabase.rs`
//   2. Adapte structs/endpoints ao novo dominio (remova Medicao/Tomada)
//   3. Adicione `pub mod supabase;` aqui
