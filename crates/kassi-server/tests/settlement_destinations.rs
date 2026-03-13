mod common;

use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use kassi_db::diesel::prelude::*;
use kassi_db::diesel_async::RunQueryDsl;
use kassi_db::schema;
use kassi_server::AppState;
use tower::ServiceExt;

use common::{authenticate_with_key, eip191_sign, TestContext};

async fn request(
    state: &AppState,
    method: &str,
    path: &str,
    token: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let builder = match method {
        "GET" => Request::get(path),
        "POST" => Request::post(path),
        "DELETE" => Request::delete(path),
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

    let resp = kassi_server::app(state.clone()).oneshot(req).await.unwrap();

    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&body).unwrap_or_default())
}

async fn seed_network(state: &AppState, network_id: &str, display_name: &str) {
    let mut conn = state.db.get().await.unwrap();
    kassi_db::diesel::insert_into(schema::networks::table)
        .values((
            schema::networks::id.eq(network_id),
            schema::networks::display_name.eq(display_name),
            schema::networks::block_time_ms.eq(12000),
            schema::networks::confirmations.eq(12),
            schema::networks::is_active.eq(true),
        ))
        .on_conflict(schema::networks::id)
        .do_nothing()
        .execute(&mut conn)
        .await
        .unwrap();
}

fn sign_create_message(
    key: &k256::ecdsa::SigningKey,
    address: &str,
    network_ids: &[&str],
) -> String {
    let networks = network_ids.join(", ");
    let message = format!(
        "I confirm setting {address} as the settlement destination for networks: {networks}"
    );
    eip191_sign(&message, key)
}

fn sign_delete_message(key: &k256::ecdsa::SigningKey, id: &str) -> String {
    let message = format!("I confirm removing settlement destination {id}");
    eip191_sign(&message, key)
}

mod create {
    use super::*;

    #[tokio::test]
    async fn creates_settlement_destination() {
        let ctx = TestContext::new().await;
        let (token, _, key) = authenticate_with_key(&ctx.state).await;

        seed_network(&ctx.state, "eip155:1", "Ethereum").await;

        let sig = sign_create_message(
            &key,
            "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
            &["eip155:1"],
        );

        let (status, json) = request(
            &ctx.state,
            "POST",
            "/settlement-destinations",
            &token,
            Some(serde_json::json!({
                "network_ids": ["eip155:1"],
                "address": "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
                "signature": sig,
            })),
        )
        .await;

        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(json["data"][0]["network_id"], "eip155:1");
        assert_eq!(
            json["data"][0]["address"],
            "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
        );
    }

    #[tokio::test]
    async fn creates_multiple_destinations_same_namespace() {
        let ctx = TestContext::new().await;
        let (token, _, key) = authenticate_with_key(&ctx.state).await;

        seed_network(&ctx.state, "eip155:1", "Ethereum").await;
        seed_network(&ctx.state, "eip155:8453", "Base").await;

        let sig = sign_create_message(
            &key,
            "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
            &["eip155:1", "eip155:8453"],
        );

        let (status, json) = request(
            &ctx.state,
            "POST",
            "/settlement-destinations",
            &token,
            Some(serde_json::json!({
                "network_ids": ["eip155:1", "eip155:8453"],
                "address": "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
                "signature": sig,
            })),
        )
        .await;

        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(json["data"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn rejects_mixed_namespaces() {
        let ctx = TestContext::new().await;
        let (token, _, key) = authenticate_with_key(&ctx.state).await;

        seed_network(&ctx.state, "eip155:1", "Ethereum").await;
        seed_network(
            &ctx.state,
            "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp",
            "Solana",
        )
        .await;

        let sig = sign_create_message(
            &key,
            "0xdeadbeef",
            &["eip155:1", "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp"],
        );

        let (status, json) = request(
            &ctx.state,
            "POST",
            "/settlement-destinations",
            &token,
            Some(serde_json::json!({
                "network_ids": ["eip155:1", "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp"],
                "address": "0xdeadbeef",
                "signature": sig,
            })),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"]["code"], "validation_failed");
    }

    #[tokio::test]
    async fn rejects_empty_network_ids() {
        let ctx = TestContext::new().await;
        let (token, _, _) = authenticate_with_key(&ctx.state).await;

        let (status, json) = request(
            &ctx.state,
            "POST",
            "/settlement-destinations",
            &token,
            Some(serde_json::json!({
                "network_ids": [],
                "address": "0xdeadbeef",
                "signature": "0x00",
            })),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"]["code"], "validation_failed");
    }

    #[tokio::test]
    async fn rejects_invalid_signature() {
        let ctx = TestContext::new().await;
        let (token, _, _) = authenticate_with_key(&ctx.state).await;

        seed_network(&ctx.state, "eip155:1", "Ethereum").await;

        let (status, json) = request(
            &ctx.state,
            "POST",
            "/settlement-destinations",
            &token,
            Some(serde_json::json!({
                "network_ids": ["eip155:1"],
                "address": "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
                "signature": "0x0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
            })),
        )
        .await;

        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(json["error"]["code"], "invalid_signature");
    }

    #[tokio::test]
    async fn rejects_nonexistent_network() {
        let ctx = TestContext::new().await;
        let (token, _, key) = authenticate_with_key(&ctx.state).await;

        let sig = sign_create_message(&key, "0xdeadbeef", &["eip155:999999"]);

        let (status, json) = request(
            &ctx.state,
            "POST",
            "/settlement-destinations",
            &token,
            Some(serde_json::json!({
                "network_ids": ["eip155:999999"],
                "address": "0xdeadbeef",
                "signature": sig,
            })),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"]["code"], "validation_failed");
    }

    #[tokio::test]
    async fn upserts_existing_destination() {
        let ctx = TestContext::new().await;
        let (token, _, key) = authenticate_with_key(&ctx.state).await;

        seed_network(&ctx.state, "eip155:1", "Ethereum").await;

        let addr1 = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let sig1 = sign_create_message(&key, addr1, &["eip155:1"]);

        let (status, _) = request(
            &ctx.state,
            "POST",
            "/settlement-destinations",
            &token,
            Some(serde_json::json!({
                "network_ids": ["eip155:1"],
                "address": addr1,
                "signature": sig1,
            })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);

        let addr2 = "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let sig2 = sign_create_message(&key, addr2, &["eip155:1"]);

        let (status, json) = request(
            &ctx.state,
            "POST",
            "/settlement-destinations",
            &token,
            Some(serde_json::json!({
                "network_ids": ["eip155:1"],
                "address": addr2,
                "signature": sig2,
            })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(json["data"][0]["address"], addr2);

        // list should show only one destination for this network
        let (status, json) =
            request(&ctx.state, "GET", "/settlement-destinations", &token, None).await;
        assert_eq!(status, StatusCode::OK);

        let destinations: Vec<&serde_json::Value> = json["data"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|d| d["network_id"] == "eip155:1")
            .collect();
        assert_eq!(destinations.len(), 1);
        assert_eq!(destinations[0]["address"], addr2);
    }

    #[tokio::test]
    async fn unauthenticated_returns_401() {
        let ctx = TestContext::new().await;

        let resp = kassi_server::app(ctx.state.clone())
            .oneshot(
                Request::post("/settlement-destinations")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::to_vec(&serde_json::json!({
                            "network_ids": ["eip155:1"],
                            "address": "0xdeadbeef",
                            "signature": "0x00",
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}

mod list {
    use super::*;

    #[tokio::test]
    async fn returns_empty_list_initially() {
        let ctx = TestContext::new().await;
        let (token, _, _) = authenticate_with_key(&ctx.state).await;

        let (status, json) =
            request(&ctx.state, "GET", "/settlement-destinations", &token, None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(json["data"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn returns_created_destinations() {
        let ctx = TestContext::new().await;
        let (token, _, key) = authenticate_with_key(&ctx.state).await;

        seed_network(&ctx.state, "eip155:1", "Ethereum").await;

        let sig = sign_create_message(
            &key,
            "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
            &["eip155:1"],
        );

        request(
            &ctx.state,
            "POST",
            "/settlement-destinations",
            &token,
            Some(serde_json::json!({
                "network_ids": ["eip155:1"],
                "address": "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
                "signature": sig,
            })),
        )
        .await;

        let (status, json) =
            request(&ctx.state, "GET", "/settlement-destinations", &token, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["data"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn unauthenticated_returns_401() {
        let ctx = TestContext::new().await;

        let resp = kassi_server::app(ctx.state.clone())
            .oneshot(
                Request::get("/settlement-destinations")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}

mod delete {
    use super::*;

    #[tokio::test]
    async fn deletes_settlement_destination() {
        let ctx = TestContext::new().await;
        let (token, _, key) = authenticate_with_key(&ctx.state).await;

        seed_network(&ctx.state, "eip155:1", "Ethereum").await;

        let sig = sign_create_message(
            &key,
            "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
            &["eip155:1"],
        );

        let (_, json) = request(
            &ctx.state,
            "POST",
            "/settlement-destinations",
            &token,
            Some(serde_json::json!({
                "network_ids": ["eip155:1"],
                "address": "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
                "signature": sig,
            })),
        )
        .await;

        let dest_id = json["data"][0]["id"].as_str().unwrap();
        let delete_sig = sign_delete_message(&key, dest_id);

        let (status, _) = request(
            &ctx.state,
            "DELETE",
            &format!("/settlement-destinations/{dest_id}"),
            &token,
            Some(serde_json::json!({ "signature": delete_sig })),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        // verify it's gone
        let (_, json) = request(&ctx.state, "GET", "/settlement-destinations", &token, None).await;
        assert!(json["data"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn rejects_invalid_signature() {
        let ctx = TestContext::new().await;
        let (token, _, key) = authenticate_with_key(&ctx.state).await;

        seed_network(&ctx.state, "eip155:1", "Ethereum").await;

        let sig = sign_create_message(
            &key,
            "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
            &["eip155:1"],
        );

        let (_, json) = request(
            &ctx.state,
            "POST",
            "/settlement-destinations",
            &token,
            Some(serde_json::json!({
                "network_ids": ["eip155:1"],
                "address": "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
                "signature": sig,
            })),
        )
        .await;

        let dest_id = json["data"][0]["id"].as_str().unwrap();

        let (status, json) = request(
            &ctx.state,
            "DELETE",
            &format!("/settlement-destinations/{dest_id}"),
            &token,
            Some(serde_json::json!({
                "signature": "0x0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000"
            })),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(json["error"]["code"], "invalid_signature");
    }

    #[tokio::test]
    async fn returns_404_for_nonexistent() {
        let ctx = TestContext::new().await;
        let (token, _, key) = authenticate_with_key(&ctx.state).await;

        let delete_sig = sign_delete_message(&key, "sdst_nonexistent");

        let (status, json) = request(
            &ctx.state,
            "DELETE",
            "/settlement-destinations/sdst_nonexistent",
            &token,
            Some(serde_json::json!({ "signature": delete_sig })),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json["error"]["code"], "resource_not_found");
    }

    #[tokio::test]
    async fn unauthenticated_returns_401() {
        let ctx = TestContext::new().await;

        let resp = kassi_server::app(ctx.state.clone())
            .oneshot(
                Request::delete("/settlement-destinations/sdst_123")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::to_vec(&serde_json::json!({ "signature": "0x00" })).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
