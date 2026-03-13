mod common;

use axum::http::{Request, StatusCode};
use kassi_db::diesel::prelude::*;
use kassi_db::diesel_async::RunQueryDsl;
use kassi_db::schema;
use kassi_server::AppState;
use tower::ServiceExt;

use common::{authenticate, request, seed_network, TestContext};

struct SeedAsset<'a> {
    id: &'a str,
    network_id: &'a str,
    caip19: &'a str,
    symbol: &'a str,
    name: &'a str,
    decimals: i32,
    is_active: bool,
}

async fn seed_asset(state: &AppState, asset: SeedAsset<'_>) {
    let mut conn = state.db.get().await.unwrap();
    kassi_db::diesel::insert_into(schema::assets::table)
        .values((
            schema::assets::id.eq(asset.id),
            schema::assets::network_id.eq(asset.network_id),
            schema::assets::caip19.eq(asset.caip19),
            schema::assets::symbol.eq(asset.symbol),
            schema::assets::name.eq(asset.name),
            schema::assets::decimals.eq(asset.decimals),
            schema::assets::coingecko_id.eq("usd-coin"),
            schema::assets::is_active.eq(asset.is_active),
        ))
        .on_conflict(schema::assets::id)
        .do_nothing()
        .execute(&mut conn)
        .await
        .unwrap();
}

#[tokio::test]
async fn list_assets_returns_active_assets_with_network_info() {
    let ctx = TestContext::new().await;
    let (token, _) = authenticate(&ctx.state).await;

    seed_network(&ctx.state, "eip155:8453", "Base").await;
    seed_network(
        &ctx.state,
        "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp",
        "Solana",
    )
    .await;

    seed_asset(
        &ctx.state,
        SeedAsset {
            id: "ast_usdc_base",
            network_id: "eip155:8453",
            caip19: "eip155:8453/erc20:0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
            symbol: "USDC",
            name: "USD Coin",
            decimals: 6,
            is_active: true,
        },
    )
    .await;

    seed_asset(
        &ctx.state,
        SeedAsset {
            id: "ast_usdc_sol",
            network_id: "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp",
            caip19: "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp/spl:EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            symbol: "USDC",
            name: "USD Coin",
            decimals: 6,
            is_active: true,
        },
    )
    .await;

    // inactive asset should not appear
    seed_asset(
        &ctx.state,
        SeedAsset {
            id: "ast_dai_base",
            network_id: "eip155:8453",
            caip19: "eip155:8453/erc20:0x6B175474E89094C44Da98b954EedeAC495271d0F",
            symbol: "DAI",
            name: "Dai Stablecoin",
            decimals: 18,
            is_active: false,
        },
    )
    .await;

    let (status, json) = request(&ctx.state, "GET", "/assets", &token, None).await;

    assert_eq!(status, StatusCode::OK);

    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 2);

    // verify each asset has network info
    for asset in data {
        assert!(asset["id"].as_str().unwrap().starts_with("ast_"));
        assert!(asset["caip19"].as_str().is_some());
        assert!(asset["symbol"].as_str().is_some());
        assert!(asset["name"].as_str().is_some());
        assert!(asset["decimals"].as_i64().is_some());
        assert!(asset["network"]["id"].as_str().is_some());
        assert!(asset["network"]["display_name"].as_str().is_some());
        assert!(asset["created_at"].as_str().is_some());
    }

    // verify inactive DAI is not present
    let symbols: Vec<&str> = data.iter().map(|a| a["symbol"].as_str().unwrap()).collect();
    assert!(!symbols.contains(&"DAI"));
}

#[tokio::test]
async fn list_assets_empty_when_none_exist() {
    let ctx = TestContext::new().await;
    let (token, _) = authenticate(&ctx.state).await;

    let (status, json) = request(&ctx.state, "GET", "/assets", &token, None).await;

    assert_eq!(status, StatusCode::OK);
    assert!(json["data"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn unauthenticated_returns_401() {
    let ctx = TestContext::new().await;

    let resp = kassi_server::app(ctx.state.clone())
        .oneshot(
            Request::get("/assets")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
