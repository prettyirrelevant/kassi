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
    caip19: &str,
    symbol: &str,
    coingecko_id: &str,
) {
    let mut conn = state.db.get().await.unwrap();
    kassi_db::diesel::insert_into(schema::assets::table)
        .values((
            schema::assets::id.eq(asset_id),
            schema::assets::network_id.eq(network_id),
            schema::assets::caip19.eq(caip19),
            schema::assets::symbol.eq(symbol),
            schema::assets::name.eq(symbol),
            schema::assets::decimals.eq(6),
            schema::assets::coingecko_id.eq(coingecko_id),
            schema::assets::is_active.eq(true),
        ))
        .on_conflict(schema::assets::id)
        .do_nothing()
        .execute(&mut conn)
        .await
        .unwrap();
}

async fn seed_price_cache(
    state: &AppState,
    asset_id: &str,
    fiat_currency: &str,
    price: &str,
    fetched_at: chrono::DateTime<chrono::Utc>,
) {
    let mut conn = state.db.get().await.unwrap();
    let id = kassi_types::EntityId::new(kassi_types::EntityPrefix::PriceCache).to_string();
    kassi_db::diesel::insert_into(schema::price_cache::table)
        .values((
            schema::price_cache::id.eq(&id),
            schema::price_cache::asset_id.eq(asset_id),
            schema::price_cache::fiat_currency.eq(fiat_currency),
            schema::price_cache::price.eq(price),
            schema::price_cache::source.eq("defillama"),
            schema::price_cache::fetched_at.eq(fetched_at),
        ))
        .execute(&mut conn)
        .await
        .unwrap();
}

#[tokio::test]
async fn prices_returns_cached_prices() {
    let ctx = TestContext::new().await;
    let (token, _) = authenticate(&ctx.state).await;

    seed_network(&ctx.state, "eip155:8453", "Base").await;
    let caip19 = "eip155:8453/erc20:0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48";
    seed_asset(
        &ctx.state,
        "ast_usdc_base",
        "eip155:8453",
        caip19,
        "USDC",
        "usd-coin",
    )
    .await;

    // seed a fresh cache entry (now)
    seed_price_cache(
        &ctx.state,
        "ast_usdc_base",
        "USD",
        "1.001",
        chrono::Utc::now(),
    )
    .await;

    let path = format!("/prices?assets={caip19}&fiat=USD");
    let (status, json) = request(&ctx.state, "GET", &path, &token, None).await;

    assert_eq!(status, StatusCode::OK);

    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["asset_id"].as_str().unwrap(), "ast_usdc_base");
    assert_eq!(data[0]["caip19"].as_str().unwrap(), caip19);
    assert_eq!(data[0]["symbol"].as_str().unwrap(), "USDC");
    assert_eq!(data[0]["fiat_currency"].as_str().unwrap(), "USD");
    assert_eq!(data[0]["price"].as_str().unwrap(), "1.001");
    assert_eq!(data[0]["source"].as_str().unwrap(), "defillama");
}

#[tokio::test]
async fn stale_cache_triggers_fresh_fetch() {
    let ctx = TestContext::new().await;
    let (token, _) = authenticate(&ctx.state).await;

    seed_network(&ctx.state, "eip155:8453", "Base").await;
    let caip19 = "eip155:8453/erc20:0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48";
    seed_asset(
        &ctx.state,
        "ast_usdc_base",
        "eip155:8453",
        caip19,
        "USDC",
        "usd-coin",
    )
    .await;

    // seed a stale cache entry (10 minutes ago, beyond the 5-minute TTL)
    let stale_time = chrono::Utc::now() - chrono::TimeDelta::seconds(600);
    seed_price_cache(&ctx.state, "ast_usdc_base", "USD", "0.999", stale_time).await;

    // configure fake price fetcher to return a different price
    ctx.fake_prices.set_price("usd-coin", 1.002);

    let path = format!("/prices?assets={caip19}&fiat=USD");
    let (status, json) = request(&ctx.state, "GET", &path, &token, None).await;

    assert_eq!(status, StatusCode::OK);

    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    // should get the fresh price, not the stale one
    assert_eq!(data[0]["price"].as_str().unwrap(), "1.002");
}

#[tokio::test]
async fn missing_assets_param_returns_400() {
    let ctx = TestContext::new().await;
    let (token, _) = authenticate(&ctx.state).await;

    let (status, json) = request(&ctx.state, "GET", "/prices", &token, None).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(json["error"]["message"]
        .as_str()
        .unwrap()
        .contains("assets"));
}

#[tokio::test]
async fn unknown_caip19_returns_404() {
    let ctx = TestContext::new().await;
    let (token, _) = authenticate(&ctx.state).await;

    let (status, json) = request(
        &ctx.state,
        "GET",
        "/prices?assets=eip155:1/erc20:0xnonexistent",
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
async fn unauthenticated_returns_401() {
    let ctx = TestContext::new().await;

    let resp = kassi_server::app(ctx.state.clone())
        .oneshot(
            Request::get("/prices?assets=foo&fiat=USD")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
