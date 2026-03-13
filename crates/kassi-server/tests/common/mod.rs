#![allow(dead_code, clippy::missing_panics_doc, clippy::must_use_candidate)]

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use alloy::hex;
use alloy::primitives::{keccak256, Address};
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use kassi_db::diesel_async::AsyncConnection;
use kassi_db::diesel_async::AsyncPgConnection;
use kassi_db::diesel_async::SimpleAsyncConnection;
use kassi_server::config::Config;
use kassi_server::prices::PriceFetcher;
use kassi_server::AppState;
use kassi_tokens::{PricingError, TokenPrice};
use tower::ServiceExt;

const MIGRATION_0_DIESEL_SETUP: &str =
    include_str!("../../../kassi-db/migrations/00000000000000_diesel_initial_setup/up.sql");
const MIGRATION_1_INITIAL_SCHEMA: &str =
    include_str!("../../../kassi-db/migrations/2026-03-08-141825_initial_schema/up.sql");

fn maintenance_url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL must be set")
}

fn test_config() -> Config {
    Config {
        database_url: String::new(),
        session_jwt_secret: "test-secret-that-is-long-enough-for-hs256".into(),
        api_key_prefix: "kassi:test:".into(),
        infisical_client_id: String::new(),
        infisical_client_secret: String::new(),
        infisical_project_id: String::new(),
        port: 3000,
        quote_lock_duration_secs: 1800,
    }
}

/// Test price fetcher that returns pre-configured prices.
pub struct FakePriceFetcher {
    prices: Mutex<HashMap<String, f64>>,
}

impl FakePriceFetcher {
    pub fn new() -> Self {
        Self {
            prices: Mutex::new(HashMap::new()),
        }
    }

    pub fn set_price(&self, coingecko_id: &str, usd_price: f64) {
        self.prices
            .lock()
            .unwrap()
            .insert(coingecko_id.to_string(), usd_price);
    }
}

impl PriceFetcher for FakePriceFetcher {
    fn fetch_prices(
        &self,
        coingecko_ids: &[String],
    ) -> Pin<Box<dyn Future<Output = Result<Vec<TokenPrice>, PricingError>> + Send + '_>> {
        let ids: Vec<String> = coingecko_ids.to_vec();
        Box::pin(async move {
            let prices = self.prices.lock().unwrap();
            ids.iter()
                .map(|id| {
                    prices
                        .get(id)
                        .map(|&usd_price| TokenPrice {
                            coingecko_id: id.clone(),
                            usd_price,
                        })
                        .ok_or_else(|| PricingError::NotFound(id.clone()))
                })
                .collect()
        })
    }
}

/// Per-test isolated database and app state. Creates a unique database on
/// construction, drops it when the struct goes out of scope.
pub struct TestContext {
    pub state: AppState,
    pub fake_prices: Arc<FakePriceFetcher>,
    db_name: String,
    maintenance_url: String,
}

impl TestContext {
    pub async fn new() -> Self {
        Self::build(None).await
    }

    pub async fn with_kms() -> Self {
        Self::build(Some(Arc::new(kassi_signer::KmsBackend::Mock(
            kassi_signer::MockKms::new(),
        ))))
        .await
    }

    async fn build(kms: Option<Arc<kassi_signer::KmsBackend>>) -> Self {
        let base_url = maintenance_url();
        let alphabet: Vec<char> = "abcdefghijklmnopqrstuvwxyz0123456789".chars().collect();
        let db_name = format!("kassi_test_{}", nanoid::nanoid!(10, &alphabet));

        // connect to maintenance db and create the test database
        let mut conn = AsyncPgConnection::establish(&base_url)
            .await
            .expect("failed to connect to maintenance db");
        conn.batch_execute(&format!("CREATE DATABASE \"{db_name}\""))
            .await
            .expect("failed to create test database");

        // build url for the new database
        let test_url = replace_db_name(&base_url, &db_name);

        // connect to the new database and run migrations
        let mut test_conn = AsyncPgConnection::establish(&test_url)
            .await
            .expect("failed to connect to test database");
        test_conn
            .batch_execute(MIGRATION_0_DIESEL_SETUP)
            .await
            .expect("failed to run diesel setup migration");
        test_conn
            .batch_execute(MIGRATION_1_INITIAL_SCHEMA)
            .await
            .expect("failed to run initial schema migration");

        let pool = kassi_db::create_pool(&test_url)
            .await
            .expect("failed to create test pool");

        let fake_prices = Arc::new(FakePriceFetcher::new());

        Self {
            state: AppState {
                db: pool,
                config: test_config(),
                kms,
                prices: fake_prices.clone(),
            },
            fake_prices,
            db_name,
            maintenance_url: base_url,
        }
    }
}

impl Drop for TestContext {
    fn drop(&mut self) {
        let url = self.maintenance_url.clone();
        let db_name = self.db_name.clone();

        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build cleanup runtime");
            rt.block_on(async {
                if let Ok(mut conn) = AsyncPgConnection::establish(&url).await {
                    let _ = conn
                        .batch_execute(&format!(
                            "SELECT pg_terminate_backend(pid) \
                             FROM pg_stat_activity \
                             WHERE datname = '{db_name}' AND pid <> pg_backend_pid()"
                        ))
                        .await;
                    let _ = conn
                        .batch_execute(&format!("DROP DATABASE IF EXISTS \"{db_name}\""))
                        .await;
                }
            });
        })
        .join()
        .ok();
    }
}

/// Replace the database name in a postgres URL.
fn replace_db_name(url: &str, new_db: &str) -> String {
    let query_start = url.find('?').unwrap_or(url.len());
    let base = &url[..query_start];
    let query = &url[query_start..];

    if let Some(last_slash) = base.rfind('/') {
        format!("{}/{new_db}{query}", &base[..last_slash])
    } else {
        format!("{url}/{new_db}")
    }
}

pub fn eth_address(key: &k256::ecdsa::SigningKey) -> Address {
    let pubkey = key.verifying_key().to_encoded_point(false);
    Address::from_raw_public_key(&pubkey.as_bytes()[1..])
}

pub fn eip191_sign(message: &str, key: &k256::ecdsa::SigningKey) -> String {
    let prefixed = format!("\x19Ethereum Signed Message:\n{}{}", message.len(), message);
    let hash = keccak256(prefixed.as_bytes());
    let (sig, recid) = key.sign_prehash_recoverable(hash.as_slice()).unwrap();
    let mut bytes = [0u8; 65];
    bytes[..64].copy_from_slice(&sig.to_bytes());
    bytes[64] = recid.to_byte() + 27;
    format!("0x{}", hex::encode(bytes))
}

pub fn siwe_message(address: &Address, nonce: &str) -> String {
    let addr = address.to_checksum(None);
    format!(
        "localhost wants you to sign in with your Ethereum account:\n\
         {addr}\n\
         \n\
         Sign in to Kassi\n\
         \n\
         URI: http://localhost\n\
         Version: 1\n\
         Chain ID: 1\n\
         Nonce: {nonce}\n\
         Issued At: 2026-01-01T00:00:00Z"
    )
}

pub fn siws_message(address: &str, nonce: &str) -> String {
    format!(
        "localhost wants you to sign in with your Solana account:\n\
         {address}\n\
         \n\
         Sign in to Kassi\n\
         \n\
         URI: http://localhost\n\
         Version: 1\n\
         Chain ID: mainnet\n\
         Nonce: {nonce}\n\
         Issued At: 2026-01-01T00:00:00Z"
    )
}

/// Signs in with a fresh EVM wallet and returns `(token, merchant_id)`.
pub async fn authenticate(state: &AppState) -> (String, String) {
    let (token, merchant_id, _) = authenticate_with_key(state).await;
    (token, merchant_id)
}

/// Signs in with a fresh EVM wallet and returns `(token, merchant_id, signing_key)`.
pub async fn authenticate_with_key(state: &AppState) -> (String, String, k256::ecdsa::SigningKey) {
    let key = k256::ecdsa::SigningKey::random(&mut rand_core::OsRng);
    let address = eth_address(&key);

    let resp = kassi_server::app(state.clone())
        .oneshot(
            Request::get("/auth/nonce")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let nonce = json["data"]["nonce"].as_str().unwrap().to_string();

    let message = siwe_message(&address, &nonce);
    let signature = eip191_sign(&message, &key);
    let payload = serde_json::json!({ "message": message, "signature": signature });

    let resp = kassi_server::app(state.clone())
        .oneshot(
            Request::post("/auth/verify")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&payload).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    (
        json["data"]["token"].as_str().unwrap().to_string(),
        json["data"]["merchant_id"].as_str().unwrap().to_string(),
        key,
    )
}
