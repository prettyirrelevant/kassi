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

async fn request(
    state: &AppState,
    method: &str,
    path: &str,
    token: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let builder = match method {
        "GET" => Request::get(path),
        "PATCH" => Request::patch(path),
        "POST" => Request::post(path),
        _ => panic!("unsupported method"),
    };

    let builder = builder.header("authorization", format!("Bearer {token}"));

    let req = if let Some(json) = body {
        builder
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_vec(&json).unwrap()))
            .unwrap()
    } else {
        builder.body(axum::body::Body::empty()).unwrap()
    };

    let resp = kassi_server::app(state.clone())
        .oneshot(req)
        .await
        .unwrap();

    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&body).unwrap())
}

async fn request_with_api_key(
    state: &AppState,
    method: &str,
    path: &str,
    api_key: &str,
) -> (StatusCode, serde_json::Value) {
    let builder = match method {
        "GET" => Request::get(path),
        _ => panic!("unsupported method"),
    };

    let resp = kassi_server::app(state.clone())
        .oneshot(
            builder
                .header("x-api-key", api_key)
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

        let (status, _) = request(&state, "POST", "/merchants/me/rotate-key", &token, None).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn valid_jwt_for_unknown_merchant_auto_creates() {
        let state = test_state().await;
        let token = make_jwt(&state, "mer_nonexistent_test_12345", false);

        let (status, json) =
            request(&state, "POST", "/merchants/me/rotate-key", &token, None).await;
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

        let (status, json) =
            request(&state, "POST", "/merchants/me/rotate-key", &token, None).await;
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

        let (_, json) =
            request(&state, "POST", "/merchants/me/rotate-key", &token, None).await;
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

        let (status, json) =
            request(&state, "POST", "/merchants/me/rotate-key", &token, None).await;
        assert_eq!(status, StatusCode::OK);

        let api_key = json["data"]["api_key"].as_str().unwrap();
        assert!(api_key.starts_with("kassi:test:"));
    }

    #[tokio::test]
    async fn old_key_stops_working_after_rotation() {
        let state = test_state().await;
        let (token, _) = authenticate(&state).await;

        let (_, json1) =
            request(&state, "POST", "/merchants/me/rotate-key", &token, None).await;
        let old_key = json1["data"]["api_key"].as_str().unwrap().to_string();

        let (_, json2) =
            request(&state, "POST", "/merchants/me/rotate-key", &token, None).await;
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

mod get_merchant {
    use super::*;

    #[tokio::test]
    async fn returns_merchant_data_with_session_auth() {
        let state = test_state().await;
        let (token, merchant_id) = authenticate(&state).await;

        let (status, json) = request(&state, "GET", "/merchants/me", &token, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["data"]["id"].as_str().unwrap(), merchant_id);
        assert!(json["data"]["created_at"].is_string());
        assert!(json["data"]["updated_at"].is_string());
    }

    #[tokio::test]
    async fn returns_merchant_data_with_api_key() {
        let state = test_state().await;
        let (token, merchant_id) = authenticate(&state).await;

        let (_, json) =
            request(&state, "POST", "/merchants/me/rotate-key", &token, None).await;
        let api_key = json["data"]["api_key"].as_str().unwrap();

        let (status, json) =
            request_with_api_key(&state, "GET", "/merchants/me", api_key).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["data"]["id"].as_str().unwrap(), merchant_id);
    }

    #[tokio::test]
    async fn unauthenticated_returns_401() {
        let state = test_state().await;

        let resp = kassi_server::app(state.clone())
            .oneshot(
                Request::get("/merchants/me")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}

mod update_merchant {
    use super::*;

    #[tokio::test]
    async fn updates_name() {
        let state = test_state().await;
        let (token, merchant_id) = authenticate(&state).await;

        let (status, json) = request(
            &state,
            "PATCH",
            "/merchants/me",
            &token,
            Some(serde_json::json!({ "name": "acme corp" })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["data"]["id"].as_str().unwrap(), merchant_id);
        assert_eq!(json["data"]["name"].as_str().unwrap(), "acme corp");
    }

    #[tokio::test]
    async fn updates_webhook_url() {
        let state = test_state().await;
        let (token, _) = authenticate(&state).await;

        let (status, json) = request(
            &state,
            "PATCH",
            "/merchants/me",
            &token,
            Some(serde_json::json!({ "webhook_url": "https://example.com/webhook" })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            json["data"]["webhook_url"].as_str().unwrap(),
            "https://example.com/webhook"
        );
    }

    #[tokio::test]
    async fn updates_both_name_and_webhook_url() {
        let state = test_state().await;
        let (token, _) = authenticate(&state).await;

        let (status, json) = request(
            &state,
            "PATCH",
            "/merchants/me",
            &token,
            Some(serde_json::json!({ "name": "new name", "webhook_url": "https://example.com/hook" })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["data"]["name"].as_str().unwrap(), "new name");
        assert_eq!(
            json["data"]["webhook_url"].as_str().unwrap(),
            "https://example.com/hook"
        );
    }

    #[tokio::test]
    async fn get_reflects_updated_name() {
        let state = test_state().await;
        let (token, _) = authenticate(&state).await;

        request(
            &state,
            "PATCH",
            "/merchants/me",
            &token,
            Some(serde_json::json!({ "name": "persistent name" })),
        )
        .await;

        let (status, json) = request(&state, "GET", "/merchants/me", &token, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["data"]["name"].as_str().unwrap(), "persistent name");
    }

    #[tokio::test]
    async fn unauthenticated_returns_401() {
        let state = test_state().await;

        let resp = kassi_server::app(state.clone())
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/merchants/me")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::to_vec(&serde_json::json!({ "name": "nope" })).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}

mod rotate_webhook_secret {
    use super::*;

    #[tokio::test]
    async fn returns_new_secret() {
        let state = test_state().await;
        let (token, _) = authenticate(&state).await;

        let (status, json) =
            request(&state, "POST", "/merchants/me/rotate-webhook-secret", &token, None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(json["data"]["webhook_secret"].is_string());
        assert!(!json["data"]["webhook_secret"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn returns_different_secret_each_time() {
        let state = test_state().await;
        let (token, _) = authenticate(&state).await;

        let (_, json1) =
            request(&state, "POST", "/merchants/me/rotate-webhook-secret", &token, None).await;
        let (_, json2) =
            request(&state, "POST", "/merchants/me/rotate-webhook-secret", &token, None).await;

        assert_ne!(
            json1["data"]["webhook_secret"].as_str().unwrap(),
            json2["data"]["webhook_secret"].as_str().unwrap(),
        );
    }

    #[tokio::test]
    async fn unauthenticated_returns_401() {
        let state = test_state().await;

        let resp = kassi_server::app(state.clone())
            .oneshot(
                Request::post("/merchants/me/rotate-webhook-secret")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
