mod common;

use axum::http::{Request, StatusCode};
use kassi_db::diesel::prelude::*;
use kassi_db::diesel_async::RunQueryDsl;
use kassi_db::schema;
use kassi_server::AppState;
use tower::ServiceExt;

use common::{request_basic_auth, seed_network, TestContext};

const USERNAME: &str = "kassi";
const INTERNAL_PASSWORD: &str = "internal-secret";

async fn seed_network_address(state: &AppState, network_id: &str, address: &str) {
    let mut conn = state.db.get().await.unwrap();

    // create a minimal deposit address to own the network address
    let dep_id = format!("dep_{}", nanoid::nanoid!(10));
    // create a throwaway merchant
    let mer_id = format!("mer_{}", nanoid::nanoid!(10));
    kassi_db::diesel::insert_into(schema::merchants::table)
        .values(schema::merchants::id.eq(&mer_id))
        .on_conflict(schema::merchants::id)
        .do_nothing()
        .execute(&mut conn)
        .await
        .unwrap();

    kassi_db::diesel::insert_into(schema::deposit_addresses::table)
        .values((
            schema::deposit_addresses::id.eq(&dep_id),
            schema::deposit_addresses::merchant_id.eq(&mer_id),
            schema::deposit_addresses::address_type.eq("reusable"),
        ))
        .execute(&mut conn)
        .await
        .unwrap();

    let nadr_id = format!("nadr_{}", nanoid::nanoid!(10));
    kassi_db::diesel::insert_into(schema::network_addresses::table)
        .values((
            schema::network_addresses::id.eq(&nadr_id),
            schema::network_addresses::deposit_address_id.eq(&dep_id),
            schema::network_addresses::network_id.eq(network_id),
            schema::network_addresses::address.eq(address),
            schema::network_addresses::derivation_index.eq(0),
        ))
        .execute(&mut conn)
        .await
        .unwrap();
}

mod deposits {
    use super::*;

    #[tokio::test]
    async fn deposit_for_known_address_returns_200() {
        let ctx = TestContext::new().await;
        seed_network(&ctx.state, "eip155:1", "Ethereum").await;
        seed_network_address(&ctx.state, "eip155:1", "0xabc123").await;

        let (status, json) = request_basic_auth(
            &ctx.state,
            "POST",
            "/internal/deposits",
            USERNAME,
            INTERNAL_PASSWORD,
            Some(serde_json::json!({
                "network_id": "eip155:1",
                "tx_hash": "0xtxhash",
                "from_address": "0xsender",
                "to_address": "0xabc123",
                "amount": "1000000",
                "token_address": "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
                "block_number": 12345
            })),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["data"]["status"].as_str().unwrap(), "accepted");

        // verify a deposit job was enqueued
        let mut conn = ctx.state.db.get().await.unwrap();
        let jobs: Vec<(String, serde_json::Value)> = schema::jobs::table
            .filter(schema::jobs::queue.eq("deposits"))
            .filter(schema::jobs::status.eq("pending"))
            .select((schema::jobs::queue, schema::jobs::payload))
            .load::<(String, serde_json::Value)>(&mut conn)
            .await
            .unwrap();

        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].1["to_address"].as_str().unwrap(), "0xabc123");
        assert_eq!(jobs[0].1["block_number"].as_i64().unwrap(), 12345);
    }

    #[tokio::test]
    async fn deposit_for_unknown_address_returns_404() {
        let ctx = TestContext::new().await;
        seed_network(&ctx.state, "eip155:1", "Ethereum").await;

        let (status, json) = request_basic_auth(
            &ctx.state,
            "POST",
            "/internal/deposits",
            USERNAME,
            INTERNAL_PASSWORD,
            Some(serde_json::json!({
                "network_id": "eip155:1",
                "tx_hash": "0xtxhash",
                "from_address": "0xsender",
                "to_address": "0xunknown",
                "amount": "1000000",
                "token_address": "0xtoken",
                "block_number": 100
            })),
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            json["error"]["code"].as_str().unwrap(),
            "resource_not_found"
        );
    }

    #[tokio::test]
    async fn missing_basic_auth_returns_401() {
        let ctx = TestContext::new().await;

        let resp = kassi_server::app(ctx.state.clone())
            .oneshot(
                Request::post("/internal/deposits")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::to_vec(&serde_json::json!({
                            "network_id": "eip155:1",
                            "tx_hash": "0x",
                            "from_address": "0x",
                            "to_address": "0x",
                            "amount": "1",
                            "token_address": "0x",
                            "block_number": 1
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn wrong_password_returns_401() {
        let ctx = TestContext::new().await;

        let (status, _) = request_basic_auth(
            &ctx.state,
            "POST",
            "/internal/deposits",
            USERNAME,
            "wrong-password",
            Some(serde_json::json!({
                "network_id": "eip155:1",
                "tx_hash": "0x",
                "from_address": "0x",
                "to_address": "0x",
                "amount": "1",
                "token_address": "0x",
                "block_number": 1
            })),
        )
        .await;

        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn admin_password_does_not_work_for_internal() {
        let ctx = TestContext::new().await;

        let (status, _) = request_basic_auth(
            &ctx.state,
            "POST",
            "/internal/deposits",
            USERNAME,
            "admin-secret",
            Some(serde_json::json!({
                "network_id": "eip155:1",
                "tx_hash": "0x",
                "from_address": "0x",
                "to_address": "0x",
                "amount": "1",
                "token_address": "0x",
                "block_number": 1
            })),
        )
        .await;

        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn missing_fields_returns_400() {
        let ctx = TestContext::new().await;

        let (status, json) = request_basic_auth(
            &ctx.state,
            "POST",
            "/internal/deposits",
            USERNAME,
            INTERNAL_PASSWORD,
            Some(serde_json::json!({})),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"]["code"].as_str().unwrap(), "validation_failed");
        let details = json["error"]["details"].as_array().unwrap();
        assert_eq!(details.len(), 7);
    }
}

mod addresses {
    use super::*;

    #[tokio::test]
    async fn returns_grouped_map() {
        let ctx = TestContext::new().await;
        seed_network(&ctx.state, "eip155:1", "Ethereum").await;
        seed_network(&ctx.state, "eip155:8453", "Base").await;
        seed_network_address(&ctx.state, "eip155:1", "0xaddr1").await;
        seed_network_address(&ctx.state, "eip155:1", "0xaddr2").await;
        seed_network_address(&ctx.state, "eip155:8453", "0xaddr3").await;

        let (status, json) = request_basic_auth(
            &ctx.state,
            "GET",
            "/internal/addresses",
            USERNAME,
            INTERNAL_PASSWORD,
            None,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        let eth_addrs = json["eip155:1"].as_array().unwrap();
        assert_eq!(eth_addrs.len(), 2);
        let base_addrs = json["eip155:8453"].as_array().unwrap();
        assert_eq!(base_addrs.len(), 1);
    }

    #[tokio::test]
    async fn empty_returns_empty_map() {
        let ctx = TestContext::new().await;

        let (status, json) = request_basic_auth(
            &ctx.state,
            "GET",
            "/internal/addresses",
            USERNAME,
            INTERNAL_PASSWORD,
            None,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert!(json.as_object().unwrap().is_empty());
    }

    #[tokio::test]
    async fn missing_auth_returns_401() {
        let ctx = TestContext::new().await;

        let resp = kassi_server::app(ctx.state.clone())
            .oneshot(
                Request::get("/internal/addresses")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
