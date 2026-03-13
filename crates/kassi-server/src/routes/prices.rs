use std::collections::HashMap;

use axum::extract::{Query, State};
use axum::routing::get;
use axum::Router;
use chrono::{DateTime, TimeDelta, Utc};
use diesel_async::AsyncPgConnection;
use kassi_db::diesel_async;
use kassi_db::models::Asset;
use kassi_types::{EntityId, EntityPrefix};
use serde::{Deserialize, Serialize};

use crate::errors::ServerError;
use crate::extractors::AnyAuth;
use crate::prices::PriceFetcher;
use crate::response::ApiSuccess;
use crate::AppState;

#[derive(Deserialize)]
struct PriceParams {
    assets: Option<String>,
    fiat: Option<String>,
}

#[derive(Serialize)]
struct PriceResponse {
    asset_id: String,
    caip19: String,
    symbol: String,
    fiat_currency: String,
    price: String,
    source: String,
    fetched_at: DateTime<Utc>,
}

impl PriceResponse {
    fn from_asset(
        asset: &Asset,
        fiat: &str,
        price: String,
        source: String,
        fetched_at: DateTime<Utc>,
    ) -> Self {
        Self {
            asset_id: asset.id.clone(),
            caip19: asset.caip19.clone(),
            symbol: asset.symbol.clone(),
            fiat_currency: fiat.to_string(),
            price,
            source,
            fetched_at,
        }
    }
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/prices", get(get_prices))
}

async fn get_prices(
    State(state): State<AppState>,
    _auth: AnyAuth,
    Query(params): Query<PriceParams>,
) -> Result<ApiSuccess<Vec<PriceResponse>>, ServerError> {
    let assets_param = params
        .assets
        .as_deref()
        .ok_or_else(|| ServerError::BadRequest("'assets' query parameter is required.".into()))?;

    let fiat = params.fiat.as_deref().unwrap_or("USD");

    let caip19_ids: Vec<&str> = assets_param
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();

    if caip19_ids.is_empty() {
        return Err(ServerError::BadRequest(
            "'assets' must contain at least one CAIP-19 identifier.".into(),
        ));
    }

    let mut conn = state
        .db
        .get()
        .await
        .map_err(|e| kassi_db::DbError::Pool(e.to_string()))?;

    let assets = resolve_assets(&mut conn, &caip19_ids).await?;

    let stale_threshold = Utc::now() - TimeDelta::seconds(state.config.price_cache_stale_secs);
    let (mut results, stale_assets) =
        split_fresh_and_stale(&mut conn, &assets, fiat, stale_threshold).await?;

    if !stale_assets.is_empty() {
        let fresh = fetch_and_cache(&mut conn, &stale_assets, fiat, state.prices.as_ref()).await?;
        results.extend(fresh);
    }

    // sort results to match the input order
    let caip19_order: HashMap<&str, usize> = caip19_ids
        .iter()
        .enumerate()
        .map(|(i, id)| (*id, i))
        .collect();
    results.sort_by_key(|r| {
        caip19_order
            .get(r.caip19.as_str())
            .copied()
            .unwrap_or(usize::MAX)
    });

    Ok(ApiSuccess { data: results })
}

async fn resolve_assets(
    conn: &mut AsyncPgConnection,
    caip19_ids: &[&str],
) -> Result<Vec<Asset>, ServerError> {
    let mut assets = Vec::with_capacity(caip19_ids.len());
    for caip19 in caip19_ids {
        let asset = kassi_db::queries::get_asset_by_caip19(conn, caip19)
            .await?
            .ok_or_else(|| ServerError::NotFound {
                entity: "asset",
                id: (*caip19).to_string(),
            })?;
        assets.push(asset);
    }
    Ok(assets)
}

async fn split_fresh_and_stale<'a>(
    conn: &mut AsyncPgConnection,
    assets: &'a [Asset],
    fiat: &str,
    stale_threshold: DateTime<Utc>,
) -> Result<(Vec<PriceResponse>, Vec<&'a Asset>), ServerError> {
    let mut results = Vec::with_capacity(assets.len());
    let mut stale = Vec::new();

    for asset in assets {
        if let Some(cached) = kassi_db::queries::get_latest_price(conn, &asset.id, fiat).await? {
            if cached.fetched_at >= stale_threshold {
                results.push(PriceResponse::from_asset(
                    asset,
                    fiat,
                    cached.price,
                    cached.source,
                    cached.fetched_at,
                ));
                continue;
            }
        }
        stale.push(asset);
    }

    Ok((results, stale))
}

async fn fetch_and_cache(
    conn: &mut AsyncPgConnection,
    stale_assets: &[&Asset],
    fiat: &str,
    fetcher: &dyn PriceFetcher,
) -> Result<Vec<PriceResponse>, ServerError> {
    let coingecko_ids: Vec<String> = stale_assets
        .iter()
        .filter_map(|a| a.coingecko_id.clone())
        .collect();

    if coingecko_ids.is_empty() {
        return Ok(vec![]);
    }

    let cg_to_asset: HashMap<&str, &Asset> = stale_assets
        .iter()
        .filter_map(|a| a.coingecko_id.as_deref().map(|cg| (cg, *a)))
        .collect();

    let fresh_prices = fetcher
        .fetch_prices(&coingecko_ids)
        .await
        .map_err(|e| ServerError::BadRequest(format!("price fetch failed: {e}")))?;

    let now = Utc::now();
    let mut results = Vec::new();

    for tp in &fresh_prices {
        if let Some(asset) = cg_to_asset.get(tp.coingecko_id.as_str()) {
            let price_str = tp.usd_price.to_string();
            let cache_id = EntityId::new(EntityPrefix::PriceCache).to_string();

            kassi_db::queries::upsert_price_cache(
                conn, &cache_id, &asset.id, fiat, &price_str, &tp.source, now,
            )
            .await?;

            results.push(PriceResponse::from_asset(
                asset,
                fiat,
                price_str,
                tp.source.clone(),
                now,
            ));
        }
    }

    Ok(results)
}
