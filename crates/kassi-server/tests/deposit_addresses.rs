mod common;

use axum::http::{Request, StatusCode};
use kassi_db::diesel::prelude::*;
use kassi_db::diesel_async::RunQueryDsl;
use kassi_db::schema;
use kassi_server::AppState;
use tower::ServiceExt;

use common::{authenticate, request, seed_network, TestContext};

async fn seed_inactive_network(state: &AppState, network_id: &str, display_name: &str) {
    let mut conn = state.db.get().await.unwrap();
    kassi_db::diesel::insert_into(schema::networks::table)
        .values((
            schema::networks::id.eq(network_id),
            schema::networks::display_name.eq(display_name),
            schema::networks::block_time_ms.eq(12_000),
            schema::networks::confirmations.eq(12),
            schema::networks::is_active.eq(false),
        ))
        .on_conflict(schema::networks::id)
        .do_nothing()
        .execute(&mut conn)
        .await
        .unwrap();
}

async fn seed_asset(state: &AppState, asset_id: &str, network_id: &str, symbol: &str) {
    let mut conn = state.db.get().await.unwrap();
    kassi_db::diesel::insert_into(schema::assets::table)
        .values((
            schema::assets::id.eq(asset_id),
            schema::assets::network_id.eq(network_id),
            schema::assets::caip19.eq(format!("{network_id}/slip44:60")),
            schema::assets::symbol.eq(symbol),
            schema::assets::name.eq(symbol),
            schema::assets::decimals.eq(18),
            schema::assets::is_active.eq(true),
        ))
        .on_conflict(schema::assets::id)
        .do_nothing()
        .execute(&mut conn)
        .await
        .unwrap();
}

mod create {
    use super::*;

    #[tokio::test]
    async fn returns_addresses_for_all_active_networks() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        seed_network(&ctx.state, "eip155:1", "Ethereum").await;
        seed_network(&ctx.state, "eip155:137", "Polygon").await;
        seed_inactive_network(&ctx.state, "eip155:999", "Inactive Chain").await;

        let (status, json) = request(
            &ctx.state,
            "POST",
            "/deposit-addresses",
            &token,
            Some(serde_json::json!({})),
        )
        .await;

        assert_eq!(status, StatusCode::CREATED);
        let na = json["data"]["network_addresses"].as_array().unwrap();
        assert_eq!(na.len(), 2);

        let network_ids: Vec<&str> = na
            .iter()
            .map(|n| n["network"]["id"].as_str().unwrap())
            .collect();
        assert!(network_ids.contains(&"eip155:1"));
        assert!(network_ids.contains(&"eip155:137"));
        assert!(!network_ids.contains(&"eip155:999"));

        // addresses should be valid checksummed hex
        for entry in na {
            let addr = entry["address"].as_str().unwrap();
            assert!(addr.starts_with("0x"), "address should start with 0x");
            assert_eq!(addr.len(), 42, "address should be 42 chars");
        }

        // default address_type should be "reusable"
        assert_eq!(json["data"]["address_type"].as_str().unwrap(), "reusable");
    }

    #[tokio::test]
    async fn no_active_networks_returns_400() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        // no networks seeded, so zero active networks

        let (status, json) = request(
            &ctx.state,
            "POST",
            "/deposit-addresses",
            &token,
            Some(serde_json::json!({})),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"]["code"], "bad_request");
    }

    #[tokio::test]
    async fn one_off_address_type() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        seed_network(&ctx.state, "eip155:1", "Ethereum").await;

        let (status, json) = request(
            &ctx.state,
            "POST",
            "/deposit-addresses",
            &token,
            Some(serde_json::json!({ "address_type": "one_off" })),
        )
        .await;

        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(json["data"]["address_type"].as_str().unwrap(), "one_off");
    }

    #[tokio::test]
    async fn invalid_address_type_returns_422() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        seed_network(&ctx.state, "eip155:1", "Ethereum").await;

        let (status, _) = request(
            &ctx.state,
            "POST",
            "/deposit-addresses",
            &token,
            Some(serde_json::json!({ "address_type": "invalid" })),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn derivation_index_increments() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        seed_network(&ctx.state, "eip155:1", "Ethereum").await;

        // create first deposit address
        let (_, json1) = request(
            &ctx.state,
            "POST",
            "/deposit-addresses",
            &token,
            Some(serde_json::json!({})),
        )
        .await;

        // create second deposit address
        let (_, json2) = request(
            &ctx.state,
            "POST",
            "/deposit-addresses",
            &token,
            Some(serde_json::json!({})),
        )
        .await;

        let addr1 = json1["data"]["network_addresses"][0]["address"]
            .as_str()
            .unwrap();
        let addr2 = json2["data"]["network_addresses"][0]["address"]
            .as_str()
            .unwrap();

        // different derivation index means different address
        assert_ne!(addr1, addr2, "sequential addresses should differ");
    }

    #[tokio::test]
    async fn preserves_label() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        seed_network(&ctx.state, "eip155:1", "Ethereum").await;

        let (status, json) = request(
            &ctx.state,
            "POST",
            "/deposit-addresses",
            &token,
            Some(serde_json::json!({ "label": "storefront-checkout" })),
        )
        .await;

        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(
            json["data"]["label"].as_str().unwrap(),
            "storefront-checkout"
        );
    }

    #[tokio::test]
    async fn unauthenticated_returns_401() {
        let ctx = TestContext::with_kms().await;

        let resp = kassi_server::app(ctx.state.clone())
            .oneshot(
                Request::post("/deposit-addresses")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::to_vec(&serde_json::json!({})).unwrap(),
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
    async fn empty_list() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        let (status, json) = request(&ctx.state, "GET", "/deposit-addresses", &token, None).await;

        assert_eq!(status, StatusCode::OK);
        assert!(json["data"].as_array().unwrap().is_empty());
        assert!(json["meta"]["next_page"].is_null());
    }

    #[tokio::test]
    async fn returns_created_addresses() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        seed_network(&ctx.state, "eip155:1", "Ethereum").await;

        // create two
        request(
            &ctx.state,
            "POST",
            "/deposit-addresses",
            &token,
            Some(serde_json::json!({ "label": "first" })),
        )
        .await;
        request(
            &ctx.state,
            "POST",
            "/deposit-addresses",
            &token,
            Some(serde_json::json!({ "label": "second" })),
        )
        .await;

        let (status, json) = request(&ctx.state, "GET", "/deposit-addresses", &token, None).await;

        assert_eq!(status, StatusCode::OK);
        let data = json["data"].as_array().unwrap();
        assert_eq!(data.len(), 2);

        // most recent first
        assert_eq!(data[0]["label"].as_str().unwrap(), "second");
        assert_eq!(data[1]["label"].as_str().unwrap(), "first");

        // each has network_addresses
        for item in data {
            assert!(!item["network_addresses"].as_array().unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn pagination() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        seed_network(&ctx.state, "eip155:1", "Ethereum").await;

        // create 3 deposit addresses
        for _ in 0..3 {
            request(
                &ctx.state,
                "POST",
                "/deposit-addresses",
                &token,
                Some(serde_json::json!({})),
            )
            .await;
        }

        // first page, limit=2
        let (status, json) = request(
            &ctx.state,
            "GET",
            "/deposit-addresses?limit=2",
            &token,
            None,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        let page1 = json["data"].as_array().unwrap();
        assert_eq!(page1.len(), 2);
        let next_page = json["meta"]["next_page"].as_str().unwrap();

        // second page
        let (status, json) = request(
            &ctx.state,
            "GET",
            &format!("/deposit-addresses?limit=2&page={next_page}"),
            &token,
            None,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        let page2 = json["data"].as_array().unwrap();
        assert_eq!(page2.len(), 1);
        assert!(json["meta"]["next_page"].is_null());

        // no overlap
        let page1_ids: Vec<&str> = page1.iter().map(|d| d["id"].as_str().unwrap()).collect();
        let page2_ids: Vec<&str> = page2.iter().map(|d| d["id"].as_str().unwrap()).collect();
        for id in &page2_ids {
            assert!(!page1_ids.contains(id));
        }
    }

    #[tokio::test]
    async fn scoped_to_merchant() {
        let ctx = TestContext::with_kms().await;

        seed_network(&ctx.state, "eip155:1", "Ethereum").await;

        let (token_a, _) = authenticate(&ctx.state).await;
        let (token_b, _) = authenticate(&ctx.state).await;

        request(
            &ctx.state,
            "POST",
            "/deposit-addresses",
            &token_a,
            Some(serde_json::json!({})),
        )
        .await;

        let (_, json) = request(&ctx.state, "GET", "/deposit-addresses", &token_b, None).await;

        assert!(json["data"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn unauthenticated_returns_401() {
        let ctx = TestContext::with_kms().await;

        let resp = kassi_server::app(ctx.state.clone())
            .oneshot(
                Request::get("/deposit-addresses")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}

mod get {
    use super::*;

    #[tokio::test]
    async fn returns_deposit_address_with_network_addresses() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        seed_network(&ctx.state, "eip155:1", "Ethereum").await;
        seed_network(&ctx.state, "eip155:137", "Polygon").await;

        let (_, created) = request(
            &ctx.state,
            "POST",
            "/deposit-addresses",
            &token,
            Some(serde_json::json!({ "label": "my-addr" })),
        )
        .await;

        let id = created["data"]["id"].as_str().unwrap();

        let (status, json) = request(
            &ctx.state,
            "GET",
            &format!("/deposit-addresses/{id}"),
            &token,
            None,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["data"]["id"].as_str().unwrap(), id);
        assert_eq!(json["data"]["label"].as_str().unwrap(), "my-addr");

        let nas = json["data"]["network_addresses"].as_array().unwrap();
        assert_eq!(nas.len(), 2);

        // each network_address has a network embed
        for na in nas {
            assert!(na["network"]["id"].as_str().is_some());
            assert!(na["network"]["display_name"].as_str().is_some());
            assert!(na["address"].as_str().is_some());
        }
    }

    #[tokio::test]
    async fn not_found_returns_404() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        let (status, _) = request(
            &ctx.state,
            "GET",
            "/deposit-addresses/nonexistent",
            &token,
            None,
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn other_merchants_address_returns_404() {
        let ctx = TestContext::with_kms().await;

        seed_network(&ctx.state, "eip155:1", "Ethereum").await;

        let (token_a, _) = authenticate(&ctx.state).await;
        let (token_b, _) = authenticate(&ctx.state).await;

        let (_, created) = request(
            &ctx.state,
            "POST",
            "/deposit-addresses",
            &token_a,
            Some(serde_json::json!({})),
        )
        .await;

        let id = created["data"]["id"].as_str().unwrap();

        let (status, _) = request(
            &ctx.state,
            "GET",
            &format!("/deposit-addresses/{id}"),
            &token_b,
            None,
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}

mod ledger_entries {
    use super::*;

    async fn seed_ledger_entry(
        state: &AppState,
        deposit_address_id: &str,
        asset_id: &str,
        network_id: &str,
        entry_type: &str,
        amount: &str,
        onchain_ref: &str,
    ) {
        let mut conn = state.db.get().await.unwrap();
        let id = kassi_types::EntityId::new(kassi_types::EntityPrefix::LedgerEntry).to_string();
        kassi_db::diesel::insert_into(schema::ledger_entries::table)
            .values((
                schema::ledger_entries::id.eq(&id),
                schema::ledger_entries::deposit_address_id.eq(deposit_address_id),
                schema::ledger_entries::asset_id.eq(asset_id),
                schema::ledger_entries::network_id.eq(network_id),
                schema::ledger_entries::entry_type.eq(entry_type),
                schema::ledger_entries::status.eq("confirmed"),
                schema::ledger_entries::amount.eq(amount),
                schema::ledger_entries::onchain_ref.eq(onchain_ref),
            ))
            .execute(&mut conn)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn empty_ledger() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        seed_network(&ctx.state, "eip155:1", "Ethereum").await;

        let (_, created) = request(
            &ctx.state,
            "POST",
            "/deposit-addresses",
            &token,
            Some(serde_json::json!({})),
        )
        .await;
        let id = created["data"]["id"].as_str().unwrap();

        let (status, json) = request(
            &ctx.state,
            "GET",
            &format!("/deposit-addresses/{id}/ledger-entries"),
            &token,
            None,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert!(json["data"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn returns_entries_for_deposit_address() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        seed_network(&ctx.state, "eip155:1", "Ethereum").await;
        seed_asset(&ctx.state, "asset-eth", "eip155:1", "ETH").await;

        // create two deposit addresses
        let (_, da1) = request(
            &ctx.state,
            "POST",
            "/deposit-addresses",
            &token,
            Some(serde_json::json!({})),
        )
        .await;
        let (_, da2) = request(
            &ctx.state,
            "POST",
            "/deposit-addresses",
            &token,
            Some(serde_json::json!({})),
        )
        .await;

        let da1_id = da1["data"]["id"].as_str().unwrap();
        let da2_id = da2["data"]["id"].as_str().unwrap();

        // seed entries for both
        seed_ledger_entry(
            &ctx.state,
            da1_id,
            "asset-eth",
            "eip155:1",
            "deposit",
            "1000000000000000000",
            "0xaaa111",
        )
        .await;
        seed_ledger_entry(
            &ctx.state,
            da1_id,
            "asset-eth",
            "eip155:1",
            "deposit",
            "2000000000000000000",
            "0xaaa222",
        )
        .await;
        seed_ledger_entry(
            &ctx.state,
            da2_id,
            "asset-eth",
            "eip155:1",
            "deposit",
            "500000000000000000",
            "0xbbb111",
        )
        .await;

        // list entries for da1 only
        let (status, json) = request(
            &ctx.state,
            "GET",
            &format!("/deposit-addresses/{da1_id}/ledger-entries"),
            &token,
            None,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        let entries = json["data"].as_array().unwrap();
        assert_eq!(entries.len(), 2);

        // entries have correct structure
        for entry in entries {
            assert_eq!(entry["deposit_address"]["id"].as_str().unwrap(), da1_id);
            assert_eq!(entry["asset"]["symbol"].as_str().unwrap(), "ETH");
            assert_eq!(entry["network"]["id"].as_str().unwrap(), "eip155:1");
            assert_eq!(entry["entry_type"].as_str().unwrap(), "deposit");
            assert_eq!(entry["status"].as_str().unwrap(), "confirmed");
        }
    }

    #[tokio::test]
    async fn nonexistent_deposit_address_returns_404() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        let (status, _) = request(
            &ctx.state,
            "GET",
            "/deposit-addresses/nonexistent/ledger-entries",
            &token,
            None,
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}
