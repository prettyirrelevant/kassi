use alloy::hex;
use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use kassi_db::diesel::prelude::*;
use kassi_db::diesel_async::RunQueryDsl;
use kassi_db::models::{Merchant, MerchantConfig};
use kassi_db::schema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::errors::ServerError;
use crate::extractors::{AnyAuth, SessionAuth};
use crate::response::ApiSuccess;
use crate::AppState;

#[derive(Serialize)]
struct MerchantResponse {
    id: String,
    name: Option<String>,
    webhook_url: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct UpdateMerchantRequest {
    name: Option<String>,
    webhook_url: Option<String>,
}

#[derive(Serialize)]
struct RotateKeyResponse {
    api_key: String,
}

#[derive(Serialize)]
struct RotateWebhookSecretResponse {
    webhook_secret: String,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/merchants/me", get(get_merchant).patch(update_merchant))
        .route("/merchants/me/rotate-key", post(rotate_key))
        .route(
            "/merchants/me/rotate-webhook-secret",
            post(rotate_webhook_secret),
        )
}

async fn get_merchant(
    State(state): State<AppState>,
    auth: AnyAuth,
) -> Result<ApiSuccess<MerchantResponse>, ServerError> {
    let mut conn = state
        .db
        .get()
        .await
        .map_err(|e| kassi_db::DbError::Pool(e.to_string()))?;

    let merchant = schema::merchants::table
        .filter(schema::merchants::id.eq(&auth.merchant_id))
        .select(Merchant::as_select())
        .first::<Merchant>(&mut conn)
        .await
        .optional()
        .map_err(kassi_db::DbError::from)?
        .ok_or_else(|| ServerError::NotFound {
            entity: "merchant",
            id: auth.merchant_id.clone(),
        })?;

    let config = schema::merchant_configs::table
        .filter(schema::merchant_configs::merchant_id.eq(&auth.merchant_id))
        .select(MerchantConfig::as_select())
        .first::<MerchantConfig>(&mut conn)
        .await
        .optional()
        .map_err(kassi_db::DbError::from)?;

    Ok(ApiSuccess {
        data: MerchantResponse {
            id: merchant.id,
            name: merchant.name,
            webhook_url: config.and_then(|c| c.webhook_url),
            created_at: merchant.created_at,
            updated_at: merchant.updated_at,
        },
    })
}

async fn update_merchant(
    State(state): State<AppState>,
    session: SessionAuth,
    Json(body): Json<UpdateMerchantRequest>,
) -> Result<ApiSuccess<MerchantResponse>, ServerError> {
    let mut conn = state
        .db
        .get()
        .await
        .map_err(|e| kassi_db::DbError::Pool(e.to_string()))?;

    // update merchant name if provided
    if let Some(ref name) = body.name {
        let updated = kassi_db::diesel::update(
            schema::merchants::table.filter(schema::merchants::id.eq(&session.merchant_id)),
        )
        .set((
            schema::merchants::name.eq(name),
            schema::merchants::updated_at.eq(Utc::now()),
        ))
        .execute(&mut conn)
        .await
        .map_err(kassi_db::DbError::from)?;

        if updated == 0 {
            return Err(ServerError::NotFound {
                entity: "merchant",
                id: session.merchant_id,
            });
        }
    }

    // update webhook_url if provided
    if let Some(ref webhook_url) = body.webhook_url {
        kassi_db::diesel::update(
            schema::merchant_configs::table
                .filter(schema::merchant_configs::merchant_id.eq(&session.merchant_id)),
        )
        .set((
            schema::merchant_configs::webhook_url.eq(webhook_url),
            schema::merchant_configs::updated_at.eq(Utc::now()),
        ))
        .execute(&mut conn)
        .await
        .map_err(kassi_db::DbError::from)?;
    }

    // re-fetch to return current state
    let merchant = schema::merchants::table
        .filter(schema::merchants::id.eq(&session.merchant_id))
        .select(Merchant::as_select())
        .first::<Merchant>(&mut conn)
        .await
        .map_err(kassi_db::DbError::from)?;

    let config = schema::merchant_configs::table
        .filter(schema::merchant_configs::merchant_id.eq(&session.merchant_id))
        .select(MerchantConfig::as_select())
        .first::<MerchantConfig>(&mut conn)
        .await
        .optional()
        .map_err(kassi_db::DbError::from)?;

    Ok(ApiSuccess {
        data: MerchantResponse {
            id: merchant.id,
            name: merchant.name,
            webhook_url: config.and_then(|c| c.webhook_url),
            created_at: merchant.created_at,
            updated_at: merchant.updated_at,
        },
    })
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

async fn rotate_webhook_secret(
    State(state): State<AppState>,
    session: SessionAuth,
) -> Result<ApiSuccess<RotateWebhookSecretResponse>, ServerError> {
    let new_secret = nanoid::nanoid!(32);

    let mut conn = state
        .db
        .get()
        .await
        .map_err(|e| kassi_db::DbError::Pool(e.to_string()))?;

    let updated = kassi_db::diesel::update(
        schema::merchant_configs::table
            .filter(schema::merchant_configs::merchant_id.eq(&session.merchant_id)),
    )
    .set((
        schema::merchant_configs::webhook_secret.eq(&new_secret),
        schema::merchant_configs::updated_at.eq(Utc::now()),
    ))
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
        data: RotateWebhookSecretResponse {
            webhook_secret: new_secret,
        },
    })
}
