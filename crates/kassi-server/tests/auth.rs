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

use common::{authenticate, eip191_sign, eth_address, siwe_message, siws_message, TestContext};

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
    let ctx = TestContext::new().await;
    let key = k256::ecdsa::SigningKey::random(&mut rand_core::OsRng);
    let address = eth_address(&key);

    let nonce = request_nonce(&ctx.state).await;
    let message = siwe_message(&address, &nonce);
    let signature = eip191_sign(&message, &key);

    let (status, json) = verify_request(&ctx.state, &message, &signature).await;

    assert_eq!(status, StatusCode::OK);
    assert!(json["data"]["token"].as_str().is_some());
    assert!(json["data"]["merchant_id"]
        .as_str()
        .unwrap()
        .starts_with("mer_"));
}

#[tokio::test]
async fn invalid_signature_returns_401() {
    let ctx = TestContext::new().await;
    let key = k256::ecdsa::SigningKey::random(&mut rand_core::OsRng);
    let address = eth_address(&key);
    let nonce = request_nonce(&ctx.state).await;
    let message = siwe_message(&address, &nonce);

    let wrong_key = k256::ecdsa::SigningKey::random(&mut rand_core::OsRng);
    let signature = eip191_sign(&message, &wrong_key);

    let (status, json) = verify_request(&ctx.state, &message, &signature).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(json["error"]["code"], "authentication_required");
}

#[tokio::test]
async fn reused_nonce_returns_401() {
    let ctx = TestContext::new().await;
    let key = k256::ecdsa::SigningKey::random(&mut rand_core::OsRng);
    let address = eth_address(&key);
    let nonce = request_nonce(&ctx.state).await;
    let message = siwe_message(&address, &nonce);
    let signature = eip191_sign(&message, &key);

    let (status, _) = verify_request(&ctx.state, &message, &signature).await;
    assert_eq!(status, StatusCode::OK);

    let (status, json) = verify_request(&ctx.state, &message, &signature).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(json["error"]["code"], "authentication_required");
}

#[tokio::test]
async fn first_login_creates_merchant_and_signer() {
    let ctx = TestContext::new().await;
    let key = k256::ecdsa::SigningKey::random(&mut rand_core::OsRng);
    let address = eth_address(&key);
    let nonce = request_nonce(&ctx.state).await;
    let message = siwe_message(&address, &nonce);
    let signature = eip191_sign(&message, &key);

    let (status, json) = verify_request(&ctx.state, &message, &signature).await;
    assert_eq!(status, StatusCode::OK);

    let merchant_id = json["data"]["merchant_id"].as_str().unwrap();
    let mut conn = ctx.state.db.get().await.unwrap();

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
    let ctx = TestContext::new().await;
    let key = k256::ecdsa::SigningKey::random(&mut rand_core::OsRng);
    let address = eth_address(&key);

    let nonce1 = request_nonce(&ctx.state).await;
    let message1 = siwe_message(&address, &nonce1);
    let signature1 = eip191_sign(&message1, &key);
    let (_, json1) = verify_request(&ctx.state, &message1, &signature1).await;
    let merchant_id_1 = json1["data"]["merchant_id"].as_str().unwrap().to_string();

    let nonce2 = request_nonce(&ctx.state).await;
    let message2 = siwe_message(&address, &nonce2);
    let signature2 = eip191_sign(&message2, &key);
    let (_, json2) = verify_request(&ctx.state, &message2, &signature2).await;
    let merchant_id_2 = json2["data"]["merchant_id"].as_str().unwrap().to_string();

    assert_eq!(merchant_id_1, merchant_id_2);
}

mod link {
    use super::*;

    async fn link_request(
        state: &AppState,
        token: &str,
        message: &str,
        signature: &str,
    ) -> (StatusCode, serde_json::Value) {
        let body = serde_json::json!({
            "message": message,
            "signature": signature,
        });

        let response = kassi_server::app(state.clone())
            .oneshot(
                Request::post("/auth/link")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {token}"))
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
    async fn link_solana_wallet_to_evm_authenticated_merchant() {
        let ctx = TestContext::new().await;
        let (token, _merchant_id) = authenticate(&ctx.state).await;

        // generate a solana keypair
        let mut rng_bytes = [0u8; 32];
        rand_core::OsRng.fill_bytes(&mut rng_bytes);
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&rng_bytes);
        let sol_address = bs58::encode(signing_key.verifying_key().as_bytes()).into_string();

        let nonce = request_nonce(&ctx.state).await;
        let message = siws_message(&sol_address, &nonce);
        let sig = signing_key.sign(message.as_bytes());
        let signature = bs58::encode(sig.to_bytes()).into_string();

        let (status, json) = link_request(&ctx.state, &token, &message, &signature).await;

        assert_eq!(status, StatusCode::OK);
        assert!(json["data"]["signer_id"]
            .as_str()
            .unwrap()
            .starts_with("sig_"));
        assert_eq!(json["data"]["address"].as_str().unwrap(), sol_address);
        assert_eq!(json["data"]["signer_type"].as_str().unwrap(), "solana");
    }

    #[tokio::test]
    async fn linked_wallet_can_authenticate_to_same_merchant() {
        let ctx = TestContext::new().await;
        let (token, merchant_id) = authenticate(&ctx.state).await;

        // link a solana wallet
        let mut rng_bytes = [0u8; 32];
        rand_core::OsRng.fill_bytes(&mut rng_bytes);
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&rng_bytes);
        let sol_address = bs58::encode(signing_key.verifying_key().as_bytes()).into_string();

        let nonce = request_nonce(&ctx.state).await;
        let message = siws_message(&sol_address, &nonce);
        let sig = signing_key.sign(message.as_bytes());
        let signature = bs58::encode(sig.to_bytes()).into_string();

        let (status, _) = link_request(&ctx.state, &token, &message, &signature).await;
        assert_eq!(status, StatusCode::OK);

        // now authenticate with the linked solana wallet
        let nonce2 = request_nonce(&ctx.state).await;
        let message2 = siws_message(&sol_address, &nonce2);
        let sig2 = signing_key.sign(message2.as_bytes());
        let signature2 = bs58::encode(sig2.to_bytes()).into_string();

        let (status, json) = verify_request(&ctx.state, &message2, &signature2).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["data"]["merchant_id"].as_str().unwrap(), merchant_id);
    }

    #[tokio::test]
    async fn linking_already_linked_wallet_returns_conflict() {
        let ctx = TestContext::new().await;
        let (token, _) = authenticate(&ctx.state).await;

        // link a solana wallet
        let mut rng_bytes = [0u8; 32];
        rand_core::OsRng.fill_bytes(&mut rng_bytes);
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&rng_bytes);
        let sol_address = bs58::encode(signing_key.verifying_key().as_bytes()).into_string();

        let nonce = request_nonce(&ctx.state).await;
        let message = siws_message(&sol_address, &nonce);
        let sig = signing_key.sign(message.as_bytes());
        let signature = bs58::encode(sig.to_bytes()).into_string();

        let (status, _) = link_request(&ctx.state, &token, &message, &signature).await;
        assert_eq!(status, StatusCode::OK);

        // try to link the same wallet again (from a different merchant)
        let (token2, _) = authenticate(&ctx.state).await;

        let nonce2 = request_nonce(&ctx.state).await;
        let message2 = siws_message(&sol_address, &nonce2);
        let sig2 = signing_key.sign(message2.as_bytes());
        let signature2 = bs58::encode(sig2.to_bytes()).into_string();

        let (status, json) = link_request(&ctx.state, &token2, &message2, &signature2).await;

        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(json["error"]["code"], "conflict");
    }
}

#[tokio::test]
async fn solana_nonce_flow_sign_verify_receive_jwt() {
    let ctx = TestContext::new().await;
    let mut rng_bytes = [0u8; 32];
    rand_core::OsRng.fill_bytes(&mut rng_bytes);
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&rng_bytes);
    let address = bs58::encode(signing_key.verifying_key().as_bytes()).into_string();

    let nonce = request_nonce(&ctx.state).await;
    let message = siws_message(&address, &nonce);

    let sig = signing_key.sign(message.as_bytes());
    let signature = bs58::encode(sig.to_bytes()).into_string();

    let (status, json) = verify_request(&ctx.state, &message, &signature).await;

    assert_eq!(status, StatusCode::OK);
    assert!(json["data"]["token"].as_str().is_some());
    assert!(json["data"]["merchant_id"]
        .as_str()
        .unwrap()
        .starts_with("mer_"));
}
