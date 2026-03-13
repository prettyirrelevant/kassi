mod common;

use axum::http::{Request, StatusCode};
use kassi_db::diesel::prelude::*;
use kassi_db::diesel_async::RunQueryDsl;
use kassi_db::schema;
use kassi_server::AppState;
use tower::ServiceExt;

use common::{authenticate, request, seed_network, TestContext};

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

/// Create a payment intent and return (`pi_id`, `deposit_address_id`).
async fn create_payment_intent(
    state: &AppState,
    token: &str,
    fake_prices: &common::FakePriceFetcher,
) -> (String, String) {
    seed_network(state, "eip155:8453", "Base").await;
    seed_asset(state, "ast_usdc_base", "eip155:8453", "USDC", 6, "usd-coin").await;
    fake_prices.set_price("usd-coin", 1.0);

    let (status, json) = request(
        state,
        "POST",
        "/payment-intents",
        token,
        Some(serde_json::json!({
            "asset_id": "ast_usdc_base",
            "fiat_amount": "25.00",
            "fiat_currency": "USD"
        })),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    (
        json["data"]["id"].as_str().unwrap().to_string(),
        json["data"]["deposit_address"]["id"]
            .as_str()
            .unwrap()
            .to_string(),
    )
}

/// Set a payment intent's status to "confirmed" directly in the DB.
async fn confirm_payment_intent(state: &AppState, pi_id: &str) {
    let mut conn = state.db.get().await.unwrap();
    kassi_db::diesel::update(
        schema::payment_intents::table.filter(schema::payment_intents::id.eq(pi_id)),
    )
    .set((
        schema::payment_intents::status.eq("confirmed"),
        schema::payment_intents::confirmed_at.eq(chrono::Utc::now()),
    ))
    .execute(&mut conn)
    .await
    .unwrap();
}

/// Seed a confirmed deposit ledger entry for a deposit address.
async fn seed_confirmed_deposit(
    state: &AppState,
    deposit_address_id: &str,
    payment_intent_id: Option<&str>,
    asset_id: &str,
    network_id: &str,
    amount: &str,
) {
    let mut conn = state.db.get().await.unwrap();
    let id = kassi_types::EntityId::new(kassi_types::EntityPrefix::LedgerEntry).to_string();
    let onchain_ref = format!("0xdeposit_{id}");
    kassi_db::diesel::insert_into(schema::ledger_entries::table)
        .values((
            schema::ledger_entries::id.eq(&id),
            schema::ledger_entries::deposit_address_id.eq(deposit_address_id),
            schema::ledger_entries::payment_intent_id.eq(payment_intent_id),
            schema::ledger_entries::asset_id.eq(asset_id),
            schema::ledger_entries::network_id.eq(network_id),
            schema::ledger_entries::entry_type.eq("deposit"),
            schema::ledger_entries::status.eq("confirmed"),
            schema::ledger_entries::amount.eq(amount),
            schema::ledger_entries::onchain_ref.eq(&onchain_ref),
        ))
        .execute(&mut conn)
        .await
        .unwrap();
}

mod payment_intent_refund {
    use super::*;

    #[tokio::test]
    async fn refund_enqueues_a_refund_job() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        let (pi_id, dep_id) = create_payment_intent(&ctx.state, &token, &ctx.fake_prices).await;
        confirm_payment_intent(&ctx.state, &pi_id).await;
        seed_confirmed_deposit(
            &ctx.state,
            &dep_id,
            Some(&pi_id),
            "ast_usdc_base",
            "eip155:8453",
            "25000000",
        )
        .await;

        let (status, json) = request(
            &ctx.state,
            "POST",
            &format!("/payment-intents/{pi_id}/refund"),
            &token,
            Some(serde_json::json!({
                "amount": "25000000",
                "destination": "0x1234567890abcdef1234567890abcdef12345678",
                "reason": "customer requested"
            })),
        )
        .await;

        assert_eq!(status, StatusCode::CREATED);

        let data = &json["data"];
        assert!(data["id"].as_str().unwrap().starts_with("le_"));
        assert_eq!(data["entry_type"].as_str().unwrap(), "refund");
        assert_eq!(data["status"].as_str().unwrap(), "pending");
        assert_eq!(data["amount"].as_str().unwrap(), "25000000");
        assert_eq!(
            data["destination"].as_str().unwrap(),
            "0x1234567890abcdef1234567890abcdef12345678"
        );
        assert_eq!(data["reason"].as_str().unwrap(), "customer requested");
        assert_eq!(data["payment_intent_id"].as_str().unwrap(), pi_id);
        assert_eq!(data["asset"]["symbol"].as_str().unwrap(), "USDC");
        assert_eq!(data["network"]["display_name"].as_str().unwrap(), "Base");

        // verify job was enqueued
        let mut conn = ctx.state.db.get().await.unwrap();
        let jobs: Vec<(String, serde_json::Value)> = schema::jobs::table
            .filter(schema::jobs::queue.eq("refunds"))
            .filter(schema::jobs::status.eq("pending"))
            .select((schema::jobs::queue, schema::jobs::payload))
            .load::<(String, serde_json::Value)>(&mut conn)
            .await
            .unwrap();

        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].1["amount"].as_str().unwrap(), "25000000");
        assert_eq!(jobs[0].1["payment_intent_id"].as_str().unwrap(), pi_id);
    }

    #[tokio::test]
    async fn refund_with_invalid_amount_returns_400() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        let (pi_id, _) = create_payment_intent(&ctx.state, &token, &ctx.fake_prices).await;
        confirm_payment_intent(&ctx.state, &pi_id).await;

        // negative amount
        let (status, json) = request(
            &ctx.state,
            "POST",
            &format!("/payment-intents/{pi_id}/refund"),
            &token,
            Some(serde_json::json!({
                "amount": "-100",
                "destination": "0x1234567890abcdef1234567890abcdef12345678"
            })),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"]["code"].as_str().unwrap(), "validation_failed");

        // zero amount
        let (status, _) = request(
            &ctx.state,
            "POST",
            &format!("/payment-intents/{pi_id}/refund"),
            &token,
            Some(serde_json::json!({
                "amount": "0",
                "destination": "0x1234567890abcdef1234567890abcdef12345678"
            })),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);

        // non-integer amount
        let (status, json) = request(
            &ctx.state,
            "POST",
            &format!("/payment-intents/{pi_id}/refund"),
            &token,
            Some(serde_json::json!({
                "amount": "25.5",
                "destination": "0x1234567890abcdef1234567890abcdef12345678"
            })),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"]["code"].as_str().unwrap(), "validation_failed");

        // missing fields
        let (status, json) = request(
            &ctx.state,
            "POST",
            &format!("/payment-intents/{pi_id}/refund"),
            &token,
            Some(serde_json::json!({})),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        let details = json["error"]["details"].as_array().unwrap();
        let fields: Vec<&str> = details
            .iter()
            .map(|d| d["field"].as_str().unwrap())
            .collect();
        assert!(fields.contains(&"amount"));
        assert!(fields.contains(&"destination"));
    }

    #[tokio::test]
    async fn refund_against_unconfirmed_intent_returns_409() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        let (pi_id, _) = create_payment_intent(&ctx.state, &token, &ctx.fake_prices).await;
        // intentionally NOT confirming

        let (status, json) = request(
            &ctx.state,
            "POST",
            &format!("/payment-intents/{pi_id}/refund"),
            &token,
            Some(serde_json::json!({
                "amount": "25000000",
                "destination": "0x1234567890abcdef1234567890abcdef12345678"
            })),
        )
        .await;

        assert_eq!(status, StatusCode::CONFLICT);
        let msg = json["error"]["message"].as_str().unwrap();
        assert!(msg.contains("pending"));
        assert!(msg.contains("only confirmed"));
    }

    #[tokio::test]
    async fn refund_nonexistent_intent_returns_404() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        let (status, json) = request(
            &ctx.state,
            "POST",
            "/payment-intents/pi_nonexistent/refund",
            &token,
            Some(serde_json::json!({
                "amount": "25000000",
                "destination": "0x1234567890abcdef1234567890abcdef12345678"
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
    async fn unauthenticated_returns_401() {
        let ctx = TestContext::with_kms().await;

        let resp = kassi_server::app(ctx.state.clone())
            .oneshot(
                Request::post("/payment-intents/pi_123/refund")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::to_vec(&serde_json::json!({
                            "amount": "25000000",
                            "destination": "0xabc"
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

mod deposit_address_refund {
    use super::*;

    #[tokio::test]
    async fn refund_enqueues_a_refund_job() {
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

        // create a reusable deposit address
        let (status, dep_json) = request(
            &ctx.state,
            "POST",
            "/deposit-addresses",
            &token,
            Some(serde_json::json!({ "address_type": "reusable" })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let dep_id = dep_json["data"]["id"].as_str().unwrap();

        // seed a confirmed deposit
        seed_confirmed_deposit(
            &ctx.state,
            dep_id,
            None,
            "ast_usdc_base",
            "eip155:8453",
            "50000000",
        )
        .await;

        let (status, json) = request(
            &ctx.state,
            "POST",
            &format!("/deposit-addresses/{dep_id}/refund"),
            &token,
            Some(serde_json::json!({
                "amount": "50000000",
                "destination": "0xdeadbeef",
                "reason": "wrong amount"
            })),
        )
        .await;

        assert_eq!(status, StatusCode::CREATED);

        let data = &json["data"];
        assert!(data["id"].as_str().unwrap().starts_with("le_"));
        assert_eq!(data["entry_type"].as_str().unwrap(), "refund");
        assert_eq!(data["status"].as_str().unwrap(), "pending");
        assert_eq!(data["amount"].as_str().unwrap(), "50000000");
        assert!(data["payment_intent_id"].is_null());
        assert_eq!(data["reason"].as_str().unwrap(), "wrong amount");

        // verify job was enqueued
        let mut conn = ctx.state.db.get().await.unwrap();
        let jobs: Vec<serde_json::Value> = schema::jobs::table
            .filter(schema::jobs::queue.eq("refunds"))
            .select(schema::jobs::payload)
            .load::<serde_json::Value>(&mut conn)
            .await
            .unwrap();

        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0]["amount"].as_str().unwrap(), "50000000");
        assert!(jobs[0]["payment_intent_id"].is_null());
    }

    #[tokio::test]
    async fn refund_without_confirmed_deposit_returns_409() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        seed_network(&ctx.state, "eip155:8453", "Base").await;

        // create deposit address but no deposits
        let (status, dep_json) = request(
            &ctx.state,
            "POST",
            "/deposit-addresses",
            &token,
            Some(serde_json::json!({ "address_type": "reusable" })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let dep_id = dep_json["data"]["id"].as_str().unwrap();

        let (status, json) = request(
            &ctx.state,
            "POST",
            &format!("/deposit-addresses/{dep_id}/refund"),
            &token,
            Some(serde_json::json!({
                "amount": "10000000",
                "destination": "0xabc"
            })),
        )
        .await;

        assert_eq!(status, StatusCode::CONFLICT);
        let msg = json["error"]["message"].as_str().unwrap();
        assert!(msg.contains("no confirmed deposits"));
    }

    #[tokio::test]
    async fn refund_nonexistent_address_returns_404() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        let (status, json) = request(
            &ctx.state,
            "POST",
            "/deposit-addresses/dep_nonexistent/refund",
            &token,
            Some(serde_json::json!({
                "amount": "10000000",
                "destination": "0xabc"
            })),
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            json["error"]["code"].as_str().unwrap(),
            "resource_not_found"
        );
    }
}

mod list {
    use super::*;

    #[tokio::test]
    async fn list_refunds_returns_refund_entries() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        let (pi_id, dep_id) = create_payment_intent(&ctx.state, &token, &ctx.fake_prices).await;
        confirm_payment_intent(&ctx.state, &pi_id).await;
        seed_confirmed_deposit(
            &ctx.state,
            &dep_id,
            Some(&pi_id),
            "ast_usdc_base",
            "eip155:8453",
            "25000000",
        )
        .await;

        // create a refund
        let (status, _) = request(
            &ctx.state,
            "POST",
            &format!("/payment-intents/{pi_id}/refund"),
            &token,
            Some(serde_json::json!({
                "amount": "10000000",
                "destination": "0xrefund1",
                "reason": "partial refund"
            })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);

        // list refunds
        let (status, json) = request(&ctx.state, "GET", "/refunds", &token, None).await;

        assert_eq!(status, StatusCode::OK);
        let data = json["data"].as_array().unwrap();
        assert_eq!(data.len(), 1);
        assert_eq!(data[0]["entry_type"].as_str().unwrap(), "refund");
        assert_eq!(data[0]["amount"].as_str().unwrap(), "10000000");
        assert_eq!(data[0]["reason"].as_str().unwrap(), "partial refund");
        assert_eq!(data[0]["asset"]["symbol"].as_str().unwrap(), "USDC");
    }

    #[tokio::test]
    async fn list_refunds_empty_when_no_refunds() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        let (status, json) = request(&ctx.state, "GET", "/refunds", &token, None).await;

        assert_eq!(status, StatusCode::OK);
        assert!(json["data"].as_array().unwrap().is_empty());
        assert!(json["meta"]["next_page"].is_null());
    }

    #[tokio::test]
    async fn list_refunds_scoped_to_merchant() {
        let ctx = TestContext::with_kms().await;

        let (token_a, _) = authenticate(&ctx.state).await;
        let (token_b, _) = authenticate(&ctx.state).await;

        let (pi_id, dep_id) = create_payment_intent(&ctx.state, &token_a, &ctx.fake_prices).await;
        confirm_payment_intent(&ctx.state, &pi_id).await;
        seed_confirmed_deposit(
            &ctx.state,
            &dep_id,
            Some(&pi_id),
            "ast_usdc_base",
            "eip155:8453",
            "25000000",
        )
        .await;

        // merchant A creates a refund
        let (status, _) = request(
            &ctx.state,
            "POST",
            &format!("/payment-intents/{pi_id}/refund"),
            &token_a,
            Some(serde_json::json!({
                "amount": "10000000",
                "destination": "0xabc"
            })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);

        // merchant B sees no refunds
        let (status, json) = request(&ctx.state, "GET", "/refunds", &token_b, None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(json["data"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn list_refunds_excludes_non_refund_entries() {
        let ctx = TestContext::with_kms().await;
        let (token, _) = authenticate(&ctx.state).await;

        let (pi_id, dep_id) = create_payment_intent(&ctx.state, &token, &ctx.fake_prices).await;
        confirm_payment_intent(&ctx.state, &pi_id).await;

        // seed a deposit entry (not a refund)
        seed_confirmed_deposit(
            &ctx.state,
            &dep_id,
            Some(&pi_id),
            "ast_usdc_base",
            "eip155:8453",
            "25000000",
        )
        .await;

        // list refunds should be empty (deposit entries are not refunds)
        let (status, json) = request(&ctx.state, "GET", "/refunds", &token, None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(json["data"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn unauthenticated_returns_401() {
        let ctx = TestContext::with_kms().await;

        let resp = kassi_server::app(ctx.state.clone())
            .oneshot(
                Request::get("/refunds")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
