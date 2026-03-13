#![allow(dead_code, clippy::missing_panics_doc, clippy::must_use_candidate)]

use alloy::hex;
use alloy::primitives::{keccak256, Address};
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use kassi_server::config::Config;
use kassi_server::AppState;
use tower::ServiceExt;

pub async fn test_state() -> AppState {
    AppState {
        db: kassi_db::create_pool(
            &std::env::var("DATABASE_URL").expect("DATABASE_URL must be set"),
        )
        .await
        .expect("failed to create test pool"),
        config: Config {
            database_url: String::new(),
            session_jwt_secret: "test-secret-that-is-long-enough-for-hs256".into(),
            api_key_prefix: "kassi:test:".into(),
            port: 3000,
        },
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
