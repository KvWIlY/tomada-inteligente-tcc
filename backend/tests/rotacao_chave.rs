//! Testes de integração da feature 005 (US4): revogação rotaciona a chave da casa e
//! deixa de aceitar o dispositivo revogado.
//!
//! Rodam contra `TEST_DATABASE_URL` (banco `tcc_test`). Se a variável não estiver
//! definida, os testes são pulados com aviso — não quebram o `cargo test` de quem não
//! tem MySQL local.

use sqlx::mysql::MySqlPoolOptions;
use sqlx::MySqlPool;
use uuid::Uuid;

use backend::auth::senha;
use backend::db;

async fn pool() -> Option<MySqlPool> {
    let _ = dotenvy::from_filename(".env");
    let url = std::env::var("TEST_DATABASE_URL").ok()?;
    MySqlPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .ok()
}

/// Cria residência + tomada + credencial e devolve `(residencia_id, tomada_id)`.
async fn cenario(pool: &MySqlPool) -> (String, String) {
    let user_id = Uuid::new_v4().to_string();
    let residencia_id = Uuid::new_v4().to_string();
    let tomada_id = Uuid::new_v4().to_string();
    let mac = format!(
        "AA:BB:CC:{:02X}:{:02X}:{:02X}",
        rand_byte(),
        rand_byte(),
        rand_byte()
    );

    sqlx::query("INSERT INTO usuarios (id, email, senha_hash, criado_em) VALUES (?, ?, ?, NOW())")
        .bind(&user_id)
        .bind(format!("{user_id}@teste.local"))
        .bind("x")
        .execute(pool)
        .await
        .expect("insert usuario");
    sqlx::query(
        "INSERT INTO residencias (id, nome, owner_id, p_max, criado_em) VALUES (?, ?, ?, ?, NOW())",
    )
    .bind(&residencia_id)
    .bind("Casa de teste")
    .bind(&user_id)
    .bind(2000.0_f32)
    .execute(pool)
    .await
    .expect("insert residencia");

    let mut conn = pool.acquire().await.expect("conn");
    db::tomadas::inserir(&mut conn, &tomada_id, &residencia_id, &mac, "Tomada teste")
        .await
        .expect("insert tomada");
    db::credenciais::upsert_credencial(&mut conn, &tomada_id, &senha::sha256_hex("segredo"))
        .await
        .expect("insert credencial");
    db::credenciais::obter_ou_criar_residencia_key(
        &mut conn,
        &residencia_id,
        senha::gerar_segredo,
    )
    .await
    .expect("chave inicial");

    (residencia_id, tomada_id)
}

fn rand_byte() -> u8 {
    use std::time::{SystemTime, UNIX_EPOCH};
    (SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos()
        % 251) as u8
}

#[tokio::test]
async fn revogar_rotaciona_chave() {
    let Some(pool) = pool().await else {
        eprintln!("TEST_DATABASE_URL ausente — teste pulado");
        return;
    };
    let (residencia_id, tomada_id) = cenario(&pool).await;

    let antes = db::credenciais::buscar_residencia_key(&pool, &residencia_id)
        .await
        .unwrap()
        .expect("chave inicial existe");

    let mut tx = pool.begin().await.unwrap();
    let n = db::credenciais::revogar(&mut tx, &tomada_id).await.unwrap();
    assert_eq!(n, 1, "a credencial estava ativa");
    db::credenciais::rotacionar_residencia_key(&mut tx, &residencia_id, senha::gerar_segredo)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let depois = db::credenciais::buscar_residencia_key(&pool, &residencia_id)
        .await
        .unwrap()
        .expect("chave nova existe");

    assert_ne!(
        antes, depois,
        "revogar DEVE trocar a chave da casa (FR-011) — sem isso o hardware revogado \
         continua sendo aceito na malha ESP-NOW"
    );
    assert_eq!(depois.len(), antes.len(), "formato da chave preservado");
}

#[tokio::test]
async fn revogar_idempotente_nao_rotaciona_duas_vezes() {
    let Some(pool) = pool().await else {
        eprintln!("TEST_DATABASE_URL ausente — teste pulado");
        return;
    };
    let (residencia_id, tomada_id) = cenario(&pool).await;

    // 1ª revogação: afeta 1 linha -> rotaciona.
    let mut tx = pool.begin().await.unwrap();
    let n1 = db::credenciais::revogar(&mut tx, &tomada_id).await.unwrap();
    db::credenciais::rotacionar_residencia_key(&mut tx, &residencia_id, senha::gerar_segredo)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(n1, 1);

    let apos_primeira = db::credenciais::buscar_residencia_key(&pool, &residencia_id)
        .await
        .unwrap()
        .unwrap();

    // 2ª revogação (segundo clique no dashboard): 0 linhas -> a rota NÃO rotaciona.
    let mut tx = pool.begin().await.unwrap();
    let n2 = db::credenciais::revogar(&mut tx, &tomada_id).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(n2, 0, "já estava revogada");

    let apos_segunda = db::credenciais::buscar_residencia_key(&pool, &residencia_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        apos_primeira, apos_segunda,
        "revogar de novo não pode espalhar churn de chave pela casa"
    );
}

#[tokio::test]
async fn config_entrega_residencia_key() {
    let Some(pool) = pool().await else {
        eprintln!("TEST_DATABASE_URL ausente — teste pulado");
        return;
    };
    let (residencia_id, _tomada_id) = cenario(&pool).await;

    let chave = db::credenciais::buscar_residencia_key(&pool, &residencia_id)
        .await
        .unwrap();
    assert!(
        chave.is_some(),
        "o GET /config precisa ter uma chave para devolver (FR-012)"
    );
}

#[tokio::test]
async fn revogado_com_jwt_valido_nao_recebe_chave() {
    let Some(pool) = pool().await else {
        eprintln!("TEST_DATABASE_URL ausente — teste pulado");
        return;
    };
    let (_residencia_id, tomada_id) = cenario(&pool).await;

    // Estado ANTES: credencial ativa -> o handler segue.
    let cred = db::credenciais::buscar_credencial(&pool, &tomada_id)
        .await
        .unwrap()
        .unwrap();
    assert!(cred.revoked_at.is_none());

    let mut tx = pool.begin().await.unwrap();
    db::credenciais::revogar(&mut tx, &tomada_id).await.unwrap();
    tx.commit().await.unwrap();

    // Achado C1 do /speckit-analyze: o JWT do device continua VÁLIDO por até o TTL
    // (1 h). É `revoked_at` — consultado a cada requisição pelo `GET /config` — que
    // impede o dispositivo revogado de receber a chave recém-rotacionada.
    let cred = db::credenciais::buscar_credencial(&pool, &tomada_id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        cred.revoked_at.is_some(),
        "a checagem por requisição do GET /config depende deste campo"
    );
}

// ============================================================
// GET /config como fonte de hora da LAN (casa sem internet)
// ============================================================

/// O `EspSntp::new_default()` do firmware aponta para a rede publica e exige WAN.
/// Numa casa com roteador e servidor local mas SEM internet o sync nunca completa, e
/// sem hora certa o firmware neutraliza as regras horarias do MILP (`a_it=true`,
/// `pi=0.5`) e descarta o buffer pendente a cada reboot — metade do modelo ficava
/// inerte. O `GET /config` passa a servir a hora do servidor para fechar esse furo.
///
/// Primeiro teste HTTP do backend: monta o app real com `rotas_config`, o mesmo que o
/// `main.rs` usa, e bate na rota com um JWT de device legitimo.
#[actix_rt::test]
async fn config_serve_hora_do_servidor() {
    let Some(pool) = pool().await else {
        eprintln!("TEST_DATABASE_URL ausente — teste pulado");
        return;
    };
    let (residencia_id, tomada_id) = cenario(&pool).await;

    let cfg = backend::config::Config {
        database_url: String::new(),
        jwt_secret: "segredo-de-teste".to_string(),
        bind_addr: String::new(),
        jwt_ttl_secs: 3600,
        cors_origins: vec![],
    };
    let (token, _exp) =
        backend::auth::jwt::emitir_device(&tomada_id, &residencia_id, &cfg.jwt_secret, 3600);

    let app = actix_web::test::init_service(
        actix_web::App::new()
            .app_data(actix_web::web::Data::new(pool.clone()))
            .app_data(actix_web::web::Data::new(cfg))
            .configure(backend::rotas_config),
    )
    .await;

    let antes = chrono::Utc::now().timestamp();
    let req = actix_web::test::TestRequest::get()
        .uri("/config")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let corpo: serde_json::Value = actix_web::test::call_and_read_body_json(&app, req).await;
    let depois = chrono::Utc::now().timestamp();

    let now = corpo
        .get("now")
        .and_then(|v| v.as_i64())
        .expect("o GET /config precisa servir `now` — e a unica fonte de hora sem WAN");

    // Plausibilidade, nao igualdade: o firmware descarta valores implausiveis e usa
    // este numero como `offset = now - uptime`. Um `now` fora da janela da requisicao
    // carimbaria as medicoes com hora errada em vez de nao carimbar.
    assert!(
        now >= antes && now <= depois,
        "`now` ({now}) precisa cair dentro da janela da requisicao ({antes}..={depois})"
    );
}
