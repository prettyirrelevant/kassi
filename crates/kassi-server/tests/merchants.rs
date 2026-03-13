mod common;

use axum::http::{Request, StatusCode};
use chrono::{Duration, Utc};
use http_body_util::BodyExt;
use jsonwebtoken::{encode, EncodingKey, Header};
use kassi_server::extractors::ApiKeyAuth;
use kassi_server::routes::auth::Claims;
use kassi_server::AppState;
use tower::ServiceExt;

use common::{authenticate, test_state};

async fn post_rotate_key(state: &AppState, token: &str) -> (StatusCode, serde_json::Value) {
    let resp = kassi_server::app(state.clone())
        .oneshot(
            Request::post("/merchants/me/rotate-key")
                .header("authorization", format!("Bearer {token}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&body).unwrap())
}

fn make_jwt(state: &AppState, merchant_id: &str, expired: bool) -> String {
    let now = Utc::now();
    let exp = if expired {
        now - Duration::hours(1)
    } else {
        now + Duration::days(7)
    };

    encode(
        &Header::default(),
        &Claims {
            merchant_id: merchant_id.into(),
            signer_address: "0xdeadbeef".into(),
            signer_type: "evm".into(),
            iat: now.timestamp(),
            exp: exp.timestamp(),
        },
        &EncodingKey::from_secret(state.config.session_jwt_secret.as_bytes()),
    )
    .unwrap()
}

fn api_key_parts(api_key: &str) -> axum::http::request::Parts {
    Request::get("/test")
        .header("x-api-key", api_key)
        .body(())
        .unwrap()
        .into_parts()
        .0
}

mod session_auth {
    use super::*;

    #[tokio::test]
    async fn valid_jwt_resolves_merchant() {
        let state = test_state().await;
        let (token, _) = authenticate(&state).await;

        let (status, _) = post_rotate_key(&state, &token).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn valid_jwt_for_unknown_merchant_auto_creates() {
        let state = test_state().await;
        let token = make_jwt(&state, "mer_nonexistent_test_12345", false);

        let (status, json) = post_rotate_key(&state, &token).await;
        assert_eq!(status, StatusCode::OK);
        assert!(json["data"]["api_key"]
            .as_str()
            .unwrap()
            .starts_with("kassi:test:"));
    }

    #[tokio::test]
    async fn expired_jwt_returns_401() {
        let state = test_state().await;
        let token = make_jwt(&state, "mer_whatever", true);

        let (status, json) = post_rotate_key(&state, &token).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(json["error"]["code"], "authentication_required");
    }

    #[tokio::test]
    async fn missing_auth_header_returns_401() {
        let state = test_state().await;

        let resp = kassi_server::app(state.clone())
            .oneshot(
                Request::post("/merchants/me/rotate-key")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}

mod api_key_auth {
    use axum::extract::FromRequestParts;

    use super::*;

    #[tokio::test]
    async fn valid_api_key_resolves_merchant() {
        let state = test_state().await;
        let (token, merchant_id) = authenticate(&state).await;

        let (_, json) = post_rotate_key(&state, &token).await;
        let api_key = json["data"]["api_key"].as_str().unwrap().to_string();

        let mut parts = api_key_parts(&api_key);
        let result = ApiKeyAuth::from_request_parts(&mut parts, &state).await;
        assert_eq!(result.unwrap().merchant_id, merchant_id);
    }

    #[tokio::test]
    async fn invalid_api_key_returns_401() {
        let state = test_state().await;

        let mut parts = api_key_parts("kassi:test:bogus_key_here");
        let result = ApiKeyAuth::from_request_parts(&mut parts, &state).await;
        assert!(result.is_err());
    }
}

mod rotate_key {
    use axum::extract::FromRequestParts;

    use super::*;

    #[tokio::test]
    async fn returns_key_with_correct_prefix() {
        let state = test_state().await;
        let (token, _) = authenticate(&state).await;

        let (status, json) = post_rotate_key(&state, &token).await;
        assert_eq!(status, StatusCode::OK);

        let api_key = json["data"]["api_key"].as_str().unwrap();
        assert!(api_key.starts_with("kassi:test:"));
    }

    #[tokio::test]
    async fn old_key_stops_working_after_rotation() {
        let state = test_state().await;
        let (token, _) = authenticate(&state).await;

        let (_, json1) = post_rotate_key(&state, &token).await;
        let old_key = json1["data"]["api_key"].as_str().unwrap().to_string();

        let (_, json2) = post_rotate_key(&state, &token).await;
        let new_key = json2["data"]["api_key"].as_str().unwrap().to_string();

        assert_ne!(old_key, new_key);

        // old key no longer resolves
        let mut parts = api_key_parts(&old_key);
        assert!(ApiKeyAuth::from_request_parts(&mut parts, &state)
            .await
            .is_err());

        // new key resolves
        let mut parts = api_key_parts(&new_key);
        assert!(ApiKeyAuth::from_request_parts(&mut parts, &state)
            .await
            .is_ok());
    }
}
