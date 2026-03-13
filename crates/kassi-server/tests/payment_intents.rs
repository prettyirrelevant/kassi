mod common;

use axum::http::{Request, StatusCode};
use chrono::Utc;
use http_body_util::BodyExt;
use kassi_db::diesel::prelude::*;
use kassi_db::diesel_async::RunQueryDsl;
use kassi_db::schema;
use kassi_server::AppState;
use tower::ServiceExt;

use common::{authenticate, TestContext};

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
            schema::networks::block_time_ms.eq(12_000),
            schema::networks::confirmations.eq(12),
            schema::networks::is_active.eq(true),
        ))
        .on_conflict(schema::networks::id)
        .do_nothing()
        .execute(&mut conn)
        .await
        .unwrap();
}

async fn seed_asset(
    state: &AppState,
    asset_id: &str,
    network_id: &str,
    symbol: &str,
    decimals: i32,
    coingecko_id: &str,
) {
    let mut conn = state.db.get().await.unwrap();
    kassi_db::diesel::insert_into(schema::assets::table)
        .values((
            schema::assets::id.eq(asset_id),
            schema::assets::network_id.eq(network_id),
            schema::assets::caip19.eq(format!(
                "{network_id}/erc20:0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"
            )),
            schema::assets::symbol.eq(symbol),
            schema::assets::name.eq(symbol),
            schema::assets::decimals.eq(decimals),
            schema::assets::coingecko_id.eq(coingecko_id),
            schema::assets::is_active.eq(true),
        ))
        .on_conflict(schema::assets::id)
        .do_nothing()
        .execute(&mut conn)
        .await
        .unwrap();
}

#[allow(clippy::too_many_arguments)]
async fn seed_ledger_entry(
    state: &AppState,
    deposit_address_id: &str,
    payment_intent_id: &str,
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
            schema::ledger_entries::payment_intent_id.eq(payment_intent_id),
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

mod create {
    use super::*;

    #[tokio::test]
    async fn returns_deposit_address_and_quote() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        seed_network(&ctx.state, "eip155:8453", "Base").await;
        seed_asset(
            &ctx.state,
            "ast_usdc_base",
            "eip155:8453",
            "USDC",
            6,
            "usd-coin",
        )
        .await;
        ctx.fake_prices.set_price("usd-coin", 1.0168);

        let (status, json) = request(
            &ctx.state,
            "POST",
            "/payment-intents",
            &token,
            Some(serde_json::json!({
                "asset_id": "ast_usdc_base",
                "fiat_amount": "25.00",
                "fiat_currency": "USD"
            })),
        )
        .await;

        assert_eq!(status, StatusCode::CREATED);

        let data = &json["data"];

        // payment intent fields
        assert!(data["id"].as_str().unwrap().starts_with("pi_"));
        assert_eq!(data["fiat_amount"].as_str().unwrap(), "25.00");
        assert_eq!(data["fiat_currency"].as_str().unwrap(), "USD");
        assert_eq!(data["status"].as_str().unwrap(), "pending");
        assert!(data["confirmed_at"].is_null());
        assert!(data["expires_at"].as_str().is_some());
        assert!(data["created_at"].as_str().is_some());

        // deposit address embed
        let dep = &data["deposit_address"];
        assert!(dep["id"].as_str().unwrap().starts_with("dep_"));
        let addr = dep["address"].as_str().unwrap();
        assert!(addr.starts_with("0x"));
        assert_eq!(addr.len(), 42);

        // quotes
        let quotes = data["quotes"].as_array().unwrap();
        assert_eq!(quotes.len(), 1);

        let quote = &quotes[0];
        assert!(quote["id"].as_str().unwrap().starts_with("quo_"));
        assert_eq!(quote["exchange_rate"].as_str().unwrap(), "1.0168");
        assert_eq!(quote["asset"]["symbol"].as_str().unwrap(), "USDC");
        assert_eq!(quote["asset"]["decimals"].as_i64().unwrap(), 6);
        assert!(quote["expires_at"].as_str().is_some());
    }

    #[tokio::test]
    async fn quote_has_correct_crypto_amount() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        seed_network(&ctx.state, "eip155:8453", "Base").await;
        seed_asset(
            &ctx.state,
            "ast_usdc_base",
            "eip155:8453",
            "USDC",
            6,
            "usd-coin",
        )
        .await;
        ctx.fake_prices.set_price("usd-coin", 1.0168);

        let (status, json) = request(
            &ctx.state,
            "POST",
            "/payment-intents",
            &token,
            Some(serde_json::json!({
                "asset_id": "ast_usdc_base",
                "fiat_amount": "25.00",
                "fiat_currency": "USD"
            })),
        )
        .await;

        assert_eq!(status, StatusCode::CREATED);

        let quote = &json["data"]["quotes"][0];
        let crypto_amount = quote["crypto_amount"].as_str().unwrap();

        // crypto_amount = floor(25.00 / 1.0168 * 1_000_000) = 24586939
        assert_eq!(crypto_amount, "24586939");
    }

    #[tokio::test]
    async fn expires_at_set_correctly() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        seed_network(&ctx.state, "eip155:8453", "Base").await;
        seed_asset(
            &ctx.state,
            "ast_usdc_base",
            "eip155:8453",
            "USDC",
            6,
            "usd-coin",
        )
        .await;
        ctx.fake_prices.set_price("usd-coin", 1.0);

        let before = Utc::now();

        let (status, json) = request(
            &ctx.state,
            "POST",
            "/payment-intents",
            &token,
            Some(serde_json::json!({
                "asset_id": "ast_usdc_base",
                "fiat_amount": "10.00",
                "fiat_currency": "USD"
            })),
        )
        .await;

        assert_eq!(status, StatusCode::CREATED);

        let expires_at_str = json["data"]["expires_at"].as_str().unwrap();
        let expires_at: chrono::DateTime<Utc> = expires_at_str.parse().unwrap();

        // quote_lock_duration_secs = 1800 (30 minutes) in test config
        let expected_min = before + chrono::Duration::seconds(1800);
        let expected_max = before + chrono::Duration::seconds(1810); // small tolerance

        assert!(
            expires_at >= expected_min && expires_at <= expected_max,
            "expires_at {expires_at} should be ~30 minutes from now ({expected_min} to {expected_max})"
        );

        // quote expires_at should match intent expires_at
        let quote_expires_at = json["data"]["quotes"][0]["expires_at"].as_str().unwrap();
        assert_eq!(expires_at_str, quote_expires_at);
    }

    #[tokio::test]
    async fn missing_required_fields_returns_validation_error() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        let (status, json) = request(
            &ctx.state,
            "POST",
            "/payment-intents",
            &token,
            Some(serde_json::json!({})),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"]["code"].as_str().unwrap(), "validation_failed");

        let details = json["error"]["details"].as_array().unwrap();
        let fields: Vec<&str> = details
            .iter()
            .map(|d| d["field"].as_str().unwrap())
            .collect();
        assert!(fields.contains(&"asset_id"));
        assert!(fields.contains(&"fiat_amount"));
        assert!(fields.contains(&"fiat_currency"));
    }

    #[tokio::test]
    async fn invalid_fiat_amount_returns_validation_error() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        let (status, body) = request(
            &ctx.state,
            "POST",
            "/payment-intents",
            &token,
            Some(serde_json::json!({
                "asset_id": "ast_usdc_base",
                "fiat_amount": "-5.00",
                "fiat_currency": "USD"
            })),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"].as_str().unwrap(), "validation_failed");
    }

    #[tokio::test]
    async fn unsupported_currency_returns_validation_error() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        let (status, json) = request(
            &ctx.state,
            "POST",
            "/payment-intents",
            &token,
            Some(serde_json::json!({
                "asset_id": "ast_usdc_base",
                "fiat_amount": "10.00",
                "fiat_currency": "BTC"
            })),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"]["code"].as_str().unwrap(), "validation_failed");
    }

    #[tokio::test]
    async fn nonexistent_asset_returns_validation_error() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        let (status, _) = request(
            &ctx.state,
            "POST",
            "/payment-intents",
            &token,
            Some(serde_json::json!({
                "asset_id": "ast_nonexistent",
                "fiat_amount": "10.00",
                "fiat_currency": "USD"
            })),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn no_price_available_returns_error() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        seed_network(&ctx.state, "eip155:8453", "Base").await;
        seed_asset(
            &ctx.state,
            "ast_usdc_base",
            "eip155:8453",
            "USDC",
            6,
            "usd-coin",
        )
        .await;
        // deliberately not setting a price in fake_prices

        let (status, json) = request(
            &ctx.state,
            "POST",
            "/payment-intents",
            &token,
            Some(serde_json::json!({
                "asset_id": "ast_usdc_base",
                "fiat_amount": "10.00",
                "fiat_currency": "USD"
            })),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        let msg = json["error"]["message"].as_str().unwrap();
        assert!(msg.contains("failed to fetch price"));
    }

    #[tokio::test]
    async fn unauthenticated_returns_401() {
        let ctx = TestContext::with_kms().await;

        let resp = kassi_server::app(ctx.state.clone())
            .oneshot(
                Request::post("/payment-intents")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::to_vec(&serde_json::json!({
                            "asset_id": "ast_usdc_base",
                            "fiat_amount": "10.00",
                            "fiat_currency": "USD"
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
    async fn caches_fetched_price() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        seed_network(&ctx.state, "eip155:8453", "Base").await;
        seed_asset(
            &ctx.state,
            "ast_usdc_base",
            "eip155:8453",
            "USDC",
            6,
            "usd-coin",
        )
        .await;
        ctx.fake_prices.set_price("usd-coin", 1.0);

        request(
            &ctx.state,
            "POST",
            "/payment-intents",
            &token,
            Some(serde_json::json!({
                "asset_id": "ast_usdc_base",
                "fiat_amount": "10.00",
                "fiat_currency": "USD"
            })),
        )
        .await;

        // verify price was cached
        let mut conn = ctx.state.db.get().await.unwrap();
        let cached: Vec<String> = schema::price_cache::table
            .filter(schema::price_cache::asset_id.eq("ast_usdc_base"))
            .filter(schema::price_cache::fiat_currency.eq("USD"))
            .select(schema::price_cache::price)
            .load::<String>(&mut conn)
            .await
            .unwrap();

        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0], "1");
    }
}

mod list {
    use super::*;

    #[tokio::test]
    async fn empty_list() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        let (status, json) = request(&ctx.state, "GET", "/payment-intents", &token, None).await;

        assert_eq!(status, StatusCode::OK);
        assert!(json["data"].as_array().unwrap().is_empty());
        assert!(json["meta"]["next_page"].is_null());
    }

    #[tokio::test]
    async fn returns_created_intents() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        seed_network(&ctx.state, "eip155:8453", "Base").await;
        seed_asset(
            &ctx.state,
            "ast_usdc_base",
            "eip155:8453",
            "USDC",
            6,
            "usd-coin",
        )
        .await;
        ctx.fake_prices.set_price("usd-coin", 1.0);

        // create two intents
        request(
            &ctx.state,
            "POST",
            "/payment-intents",
            &token,
            Some(serde_json::json!({
                "asset_id": "ast_usdc_base",
                "fiat_amount": "10.00",
                "fiat_currency": "USD"
            })),
        )
        .await;

        request(
            &ctx.state,
            "POST",
            "/payment-intents",
            &token,
            Some(serde_json::json!({
                "asset_id": "ast_usdc_base",
                "fiat_amount": "20.00",
                "fiat_currency": "USD"
            })),
        )
        .await;

        let (status, json) = request(&ctx.state, "GET", "/payment-intents", &token, None).await;

        assert_eq!(status, StatusCode::OK);
        let data = json["data"].as_array().unwrap();
        assert_eq!(data.len(), 2);

        // most recent first
        assert_eq!(data[0]["fiat_amount"].as_str().unwrap(), "20.00");
        assert_eq!(data[1]["fiat_amount"].as_str().unwrap(), "10.00");

        // each has quotes
        for item in data {
            assert!(!item["quotes"].as_array().unwrap().is_empty());
            assert!(item["deposit_address"]["id"].as_str().is_some());
            assert!(item["deposit_address"]["address"].as_str().is_some());
        }
    }

    #[tokio::test]
    async fn filter_by_status() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        seed_network(&ctx.state, "eip155:8453", "Base").await;
        seed_asset(
            &ctx.state,
            "ast_usdc_base",
            "eip155:8453",
            "USDC",
            6,
            "usd-coin",
        )
        .await;
        ctx.fake_prices.set_price("usd-coin", 1.0);

        // create an intent (will be "pending")
        let (_, created) = request(
            &ctx.state,
            "POST",
            "/payment-intents",
            &token,
            Some(serde_json::json!({
                "asset_id": "ast_usdc_base",
                "fiat_amount": "10.00",
                "fiat_currency": "USD"
            })),
        )
        .await;

        assert_eq!(created["data"]["status"].as_str().unwrap(), "pending");

        // filter by pending should return it
        let (status, json) = request(
            &ctx.state,
            "GET",
            "/payment-intents?status=pending",
            &token,
            None,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["data"].as_array().unwrap().len(), 1);

        // filter by confirmed should return empty
        let (status, json) = request(
            &ctx.state,
            "GET",
            "/payment-intents?status=confirmed",
            &token,
            None,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert!(json["data"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn invalid_status_filter_returns_error() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        let (status, json) = request(
            &ctx.state,
            "GET",
            "/payment-intents?status=bogus",
            &token,
            None,
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"]["code"].as_str().unwrap(), "validation_failed");
    }

    #[tokio::test]
    async fn scoped_to_merchant() {
        let ctx = TestContext::with_kms().await;

        seed_network(&ctx.state, "eip155:8453", "Base").await;
        seed_asset(
            &ctx.state,
            "ast_usdc_base",
            "eip155:8453",
            "USDC",
            6,
            "usd-coin",
        )
        .await;
        ctx.fake_prices.set_price("usd-coin", 1.0);

        let (token_a, _) = authenticate(&ctx.state).await;
        let (token_b, _) = authenticate(&ctx.state).await;

        request(
            &ctx.state,
            "POST",
            "/payment-intents",
            &token_a,
            Some(serde_json::json!({
                "asset_id": "ast_usdc_base",
                "fiat_amount": "10.00",
                "fiat_currency": "USD"
            })),
        )
        .await;

        let (_, json) = request(&ctx.state, "GET", "/payment-intents", &token_b, None).await;
        assert!(json["data"].as_array().unwrap().is_empty());
    }
}

mod get {
    use super::*;

    #[tokio::test]
    async fn returns_intent_with_quotes_and_ledger_entries() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        seed_network(&ctx.state, "eip155:8453", "Base").await;
        seed_asset(
            &ctx.state,
            "ast_usdc_base",
            "eip155:8453",
            "USDC",
            6,
            "usd-coin",
        )
        .await;
        ctx.fake_prices.set_price("usd-coin", 1.0);

        let (_, created) = request(
            &ctx.state,
            "POST",
            "/payment-intents",
            &token,
            Some(serde_json::json!({
                "asset_id": "ast_usdc_base",
                "fiat_amount": "10.00",
                "fiat_currency": "USD"
            })),
        )
        .await;

        let pi_id = created["data"]["id"].as_str().unwrap();
        let dep_id = created["data"]["deposit_address"]["id"].as_str().unwrap();

        // seed a ledger entry tied to this payment intent
        seed_ledger_entry(
            &ctx.state,
            dep_id,
            pi_id,
            "ast_usdc_base",
            "eip155:8453",
            "deposit",
            "10000000",
            "0xabc123",
        )
        .await;

        let (status, json) = request(
            &ctx.state,
            "GET",
            &format!("/payment-intents/{pi_id}"),
            &token,
            None,
        )
        .await;

        assert_eq!(status, StatusCode::OK);

        let data = &json["data"];
        assert_eq!(data["id"].as_str().unwrap(), pi_id);
        assert_eq!(data["fiat_amount"].as_str().unwrap(), "10.00");
        assert_eq!(data["status"].as_str().unwrap(), "pending");

        // quotes should be present
        let quotes = data["quotes"].as_array().unwrap();
        assert_eq!(quotes.len(), 1);
        assert_eq!(quotes[0]["asset"]["symbol"].as_str().unwrap(), "USDC");

        // ledger entries should be present
        let entries = data["ledger_entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["entry_type"].as_str().unwrap(), "deposit");
        assert_eq!(entries[0]["amount"].as_str().unwrap(), "10000000");
        assert_eq!(
            entries[0]["deposit_address"]["id"].as_str().unwrap(),
            dep_id
        );
    }

    #[tokio::test]
    async fn not_found_returns_404() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        let (status, json) = request(
            &ctx.state,
            "GET",
            "/payment-intents/pi_nonexistent",
            &token,
            None,
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            json["error"]["code"].as_str().unwrap(),
            "resource_not_found"
        );
    }

    #[tokio::test]
    async fn other_merchants_intent_returns_404() {
        let ctx = TestContext::with_kms().await;

        seed_network(&ctx.state, "eip155:8453", "Base").await;
        seed_asset(
            &ctx.state,
            "ast_usdc_base",
            "eip155:8453",
            "USDC",
            6,
            "usd-coin",
        )
        .await;
        ctx.fake_prices.set_price("usd-coin", 1.0);

        let (token_a, _) = authenticate(&ctx.state).await;
        let (token_b, _) = authenticate(&ctx.state).await;

        let (_, created) = request(
            &ctx.state,
            "POST",
            "/payment-intents",
            &token_a,
            Some(serde_json::json!({
                "asset_id": "ast_usdc_base",
                "fiat_amount": "10.00",
                "fiat_currency": "USD"
            })),
        )
        .await;

        let pi_id = created["data"]["id"].as_str().unwrap();

        let (status, _) = request(
            &ctx.state,
            "GET",
            &format!("/payment-intents/{pi_id}"),
            &token_b,
            None,
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn detail_without_ledger_entries() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        seed_network(&ctx.state, "eip155:8453", "Base").await;
        seed_asset(
            &ctx.state,
            "ast_usdc_base",
            "eip155:8453",
            "USDC",
            6,
            "usd-coin",
        )
        .await;
        ctx.fake_prices.set_price("usd-coin", 1.0);

        let (_, created) = request(
            &ctx.state,
            "POST",
            "/payment-intents",
            &token,
            Some(serde_json::json!({
                "asset_id": "ast_usdc_base",
                "fiat_amount": "5.00",
                "fiat_currency": "USD"
            })),
        )
        .await;

        let pi_id = created["data"]["id"].as_str().unwrap();

        let (status, json) = request(
            &ctx.state,
            "GET",
            &format!("/payment-intents/{pi_id}"),
            &token,
            None,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert!(json["data"]["ledger_entries"]
            .as_array()
            .unwrap()
            .is_empty());
        assert!(!json["data"]["quotes"].as_array().unwrap().is_empty());
    }
}
