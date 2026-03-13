use alloy::hex;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use jsonwebtoken::{decode, DecodingKey, Validation};
use kassi_db::queries::CreateMerchantParams;
use kassi_types::{EntityId, EntityPrefix};
use sha2::{Digest, Sha256};

use crate::errors::ServerError;
use crate::routes::auth::Claims;
use crate::AppState;

pub struct SessionAuth {
    pub merchant_id: String,
}

pub struct ApiKeyAuth {
    pub merchant_id: String,
}

pub struct AnyAuth {
    pub merchant_id: String,
}

impl FromRequestParts<AppState> for SessionAuth {
    type Rejection = ServerError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or(ServerError::AuthenticationRequired)?;

        let claims = decode::<Claims>(
            token,
            &DecodingKey::from_secret(state.config.session_jwt_secret.as_bytes()),
            &Validation::default(),
        )
        .map_err(|_| ServerError::AuthenticationRequired)?
        .claims;

        let mut conn = state
            .db
            .get()
            .await
            .map_err(|e| kassi_db::DbError::Pool(e.to_string()))?;

        // auto-create merchant if not present (lazy sync for test deployment)
        if !kassi_db::queries::merchant_exists(&mut conn, &claims.merchant_id).await? {
            let encrypted_seed = if let Some(kms) = &state.kms {
                Some(
                    kassi_signer::create_merchant_seed(kms, &claims.merchant_id)
                        .await
                        .map_err(|e| {
                            ServerError::BadRequest(format!("failed to create merchant seed: {e}"))
                        })?,
                )
            } else {
                None
            };

            kassi_db::queries::create_merchant_with_config(
                &mut conn,
                CreateMerchantParams {
                    merchant_id: &claims.merchant_id,
                    config_id: EntityId::new(EntityPrefix::MerchantConfig).as_ref(),
                    signer_id: EntityId::new(EntityPrefix::Signer).as_ref(),
                    encrypted_seed: encrypted_seed.as_deref(),
                    webhook_secret: &nanoid::nanoid!(32),
                    signer_address: &claims.signer_address,
                    signer_type: &claims.signer_type,
                },
            )
            .await?;
        }

        Ok(SessionAuth {
            merchant_id: claims.merchant_id,
        })
    }
}

impl FromRequestParts<AppState> for ApiKeyAuth {
    type Rejection = ServerError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let api_key = parts
            .headers
            .get("x-api-key")
            .and_then(|v| v.to_str().ok())
            .ok_or(ServerError::AuthenticationRequired)?;

        let hash = hex::encode(Sha256::digest(api_key.as_bytes()));

        let mut conn = state
            .db
            .get()
            .await
            .map_err(|e| kassi_db::DbError::Pool(e.to_string()))?;

        let config = kassi_db::queries::find_config_by_api_key_hash(&mut conn, &hash)
            .await?
            .ok_or(ServerError::AuthenticationRequired)?;

        Ok(ApiKeyAuth {
            merchant_id: config.merchant_id,
        })
    }
}

impl FromRequestParts<AppState> for AnyAuth {
    type Rejection = ServerError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if let Ok(session) = SessionAuth::from_request_parts(parts, state).await {
            return Ok(AnyAuth {
                merchant_id: session.merchant_id,
            });
        }

        let api_key = ApiKeyAuth::from_request_parts(parts, state).await?;
        Ok(AnyAuth {
            merchant_id: api_key.merchant_id,
        })
    }
}

fn verify_basic_auth(parts: &Parts, expected_token: &str) -> Result<(), ServerError> {
    let token = parts
        .headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Basic "))
        .ok_or(ServerError::AuthenticationRequired)?;

    if token == expected_token {
        Ok(())
    } else {
        Err(ServerError::AuthenticationRequired)
    }
}

pub struct InternalAuth;

impl FromRequestParts<AppState> for InternalAuth {
    type Rejection = ServerError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        verify_basic_auth(parts, &state.config.internal_basic_auth_token)?;
        Ok(Self)
    }
}

pub struct AdminAuth;

impl FromRequestParts<AppState> for AdminAuth {
    type Rejection = ServerError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        verify_basic_auth(parts, &state.config.admin_basic_auth_token)?;
        Ok(Self)
    }
}
