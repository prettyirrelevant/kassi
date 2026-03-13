use axum::extract::State;
use axum::routing::get;
use axum::Router;
use chrono::{DateTime, Utc};
use serde::Serialize;

use super::shared::NetworkEmbed;
use crate::errors::ServerError;
use crate::extractors::AnyAuth;
use crate::response::ApiSuccess;
use crate::AppState;

#[derive(Serialize)]
struct AssetResponse {
    id: String,
    caip19: String,
    symbol: String,
    name: String,
    decimals: i32,
    network: NetworkEmbed,
    created_at: DateTime<Utc>,
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/assets", get(list_assets))
}

async fn list_assets(
    State(state): State<AppState>,
    _auth: AnyAuth,
) -> Result<ApiSuccess<Vec<AssetResponse>>, ServerError> {
    let mut conn = state
        .db
        .get()
        .await
        .map_err(|e| kassi_db::DbError::Pool(e.to_string()))?;

    let rows = kassi_db::queries::get_active_assets(&mut conn).await?;

    let data = rows
        .into_iter()
        .map(|(asset, network)| AssetResponse {
            id: asset.id,
            caip19: asset.caip19,
            symbol: asset.symbol,
            name: asset.name,
            decimals: asset.decimals,
            network: NetworkEmbed::from(network),
            created_at: asset.created_at,
        })
        .collect();

    Ok(ApiSuccess { data })
}
