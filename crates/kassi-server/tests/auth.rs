mod common;

use axum::http::{Request, StatusCode};
use ed25519_dalek::Signer;
use http_body_util::BodyExt;
use kassi_db::diesel::prelude::*;
use kassi_db::diesel_async::RunQueryDsl;
use kassi_db::schema;
use kassi_server::AppState;
use rand_core::RngCore;
use tower::ServiceExt;

use common::{eip191_sign, eth_address, siwe_message, siws_message, test_state};

async fn request_nonce(state: &AppState) -> String {
    let response = kassi_server::app(state.clone())
        .oneshot(
            Request::get("/auth/nonce")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    json["data"]["nonce"].as_str().unwrap().to_string()
}

async fn verify_request(
    state: &AppState,
    message: &str,
    signature: &str,
) -> (StatusCode, serde_json::Value) {
    let body = serde_json::json!({
        "message": message,
        "signature": signature,
    });

    let response = kassi_server::app(state.clone())
        .oneshot(
            Request::post("/auth/verify")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    (status, json)
}

#[tokio::test]
async fn nonce_flow_get_nonce_sign_verify_receive_jwt() {
    let state = test_state().await;
    let key = k256::ecdsa::SigningKey::random(&mut rand_core::OsRng);
    let address = eth_address(&key);

    let nonce = request_nonce(&state).await;
    let message = siwe_message(&address, &nonce);
    let signature = eip191_sign(&message, &key);

    let (status, json) = verify_request(&state, &message, &signature).await;

    assert_eq!(status, StatusCode::OK);
    assert!(json["data"]["token"].as_str().is_some());
    assert!(json["data"]["merchant_id"]
        .as_str()
        .unwrap()
        .starts_with("mer_"));
}

#[tokio::test]
async fn invalid_signature_returns_401() {
    let state = test_state().await;
    let key = k256::ecdsa::SigningKey::random(&mut rand_core::OsRng);
    let address = eth_address(&key);
    let nonce = request_nonce(&state).await;
    let message = siwe_message(&address, &nonce);

    let wrong_key = k256::ecdsa::SigningKey::random(&mut rand_core::OsRng);
    let signature = eip191_sign(&message, &wrong_key);

    let (status, json) = verify_request(&state, &message, &signature).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(json["error"]["code"], "authentication_required");
}

#[tokio::test]
async fn reused_nonce_returns_401() {
    let state = test_state().await;
    let key = k256::ecdsa::SigningKey::random(&mut rand_core::OsRng);
    let address = eth_address(&key);
    let nonce = request_nonce(&state).await;
    let message = siwe_message(&address, &nonce);
    let signature = eip191_sign(&message, &key);

    let (status, _) = verify_request(&state, &message, &signature).await;
    assert_eq!(status, StatusCode::OK);

    let (status, json) = verify_request(&state, &message, &signature).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(json["error"]["code"], "authentication_required");
}

#[tokio::test]
async fn first_login_creates_merchant_and_signer() {
    let state = test_state().await;
    let key = k256::ecdsa::SigningKey::random(&mut rand_core::OsRng);
    let address = eth_address(&key);
    let nonce = request_nonce(&state).await;
    let message = siwe_message(&address, &nonce);
    let signature = eip191_sign(&message, &key);

    let (status, json) = verify_request(&state, &message, &signature).await;
    assert_eq!(status, StatusCode::OK);

    let merchant_id = json["data"]["merchant_id"].as_str().unwrap();
    let mut conn = state.db.get().await.unwrap();

    let merchant_count: i64 = schema::merchants::table
        .filter(schema::merchants::id.eq(merchant_id))
        .count()
        .get_result(&mut conn)
        .await
        .unwrap();
    assert_eq!(merchant_count, 1);

    let signer_count: i64 = schema::signers::table
        .filter(schema::signers::merchant_id.eq(merchant_id))
        .count()
        .get_result(&mut conn)
        .await
        .unwrap();
    assert_eq!(signer_count, 1);

    let config_count: i64 = schema::merchant_configs::table
        .filter(schema::merchant_configs::merchant_id.eq(merchant_id))
        .count()
        .get_result(&mut conn)
        .await
        .unwrap();
    assert_eq!(config_count, 1);
}

#[tokio::test]
async fn second_login_with_same_wallet_returns_same_merchant() {
    let state = test_state().await;
    let key = k256::ecdsa::SigningKey::random(&mut rand_core::OsRng);
    let address = eth_address(&key);

    let nonce1 = request_nonce(&state).await;
    let message1 = siwe_message(&address, &nonce1);
    let signature1 = eip191_sign(&message1, &key);
    let (_, json1) = verify_request(&state, &message1, &signature1).await;
    let merchant_id_1 = json1["data"]["merchant_id"].as_str().unwrap().to_string();

    let nonce2 = request_nonce(&state).await;
    let message2 = siwe_message(&address, &nonce2);
    let signature2 = eip191_sign(&message2, &key);
    let (_, json2) = verify_request(&state, &message2, &signature2).await;
    let merchant_id_2 = json2["data"]["merchant_id"].as_str().unwrap().to_string();

    assert_eq!(merchant_id_1, merchant_id_2);
}

#[tokio::test]
async fn solana_nonce_flow_sign_verify_receive_jwt() {
    let state = test_state().await;
    let mut rng_bytes = [0u8; 32];
    rand_core::OsRng.fill_bytes(&mut rng_bytes);
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&rng_bytes);
    let address = bs58::encode(signing_key.verifying_key().as_bytes()).into_string();

    let nonce = request_nonce(&state).await;
    let message = siws_message(&address, &nonce);

    let sig = signing_key.sign(message.as_bytes());
    let signature = bs58::encode(sig.to_bytes()).into_string();

    let (status, json) = verify_request(&state, &message, &signature).await;

    assert_eq!(status, StatusCode::OK);
    assert!(json["data"]["token"].as_str().is_some());
    assert!(json["data"]["merchant_id"]
        .as_str()
        .unwrap()
        .starts_with("mer_"));
}
