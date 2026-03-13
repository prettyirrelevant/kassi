use axum::extract::State;
use axum::routing::post;
use axum::Router;
use kassi_db::diesel::prelude::*;
use kassi_db::diesel_async::RunQueryDsl;
use kassi_db::schema;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::errors::ServerError;
use crate::extractors::SessionAuth;
use crate::response::ApiSuccess;
use crate::AppState;

#[derive(Serialize)]
struct RotateKeyResponse {
    api_key: String,
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/merchants/me/rotate-key", post(rotate_key))
}

async fn rotate_key(
    State(state): State<AppState>,
    session: SessionAuth,
) -> Result<ApiSuccess<RotateKeyResponse>, ServerError> {
    let raw_key = format!("{}{}", state.config.api_key_prefix, nanoid::nanoid!(32));
    let hash = hex::encode(Sha256::digest(raw_key.as_bytes()));

    let mut conn = state
        .db
        .get()
        .await
        .map_err(|e| kassi_db::DbError::Pool(e.to_string()))?;

    let updated = kassi_db::diesel::update(
        schema::merchant_configs::table
            .filter(schema::merchant_configs::merchant_id.eq(&session.merchant_id)),
    )
    .set(schema::merchant_configs::api_key_hash.eq(&hash))
    .execute(&mut conn)
    .await
    .map_err(kassi_db::DbError::from)?;

    if updated == 0 {
        return Err(ServerError::NotFound {
            entity: "merchant_config",
            id: session.merchant_id,
        });
    }

    Ok(ApiSuccess {
        data: RotateKeyResponse { api_key: raw_key },
    })
}
