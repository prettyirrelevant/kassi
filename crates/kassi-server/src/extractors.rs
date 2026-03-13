use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use jsonwebtoken::{decode, DecodingKey, Validation};
use kassi_db::diesel::prelude::*;
use kassi_db::diesel_async::RunQueryDsl;
use kassi_db::models::MerchantConfig;
use kassi_db::schema;
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

        // check if merchant exists, auto-create if not (lazy sync for test deployment)
        let exists = schema::merchants::table
            .filter(schema::merchants::id.eq(&claims.merchant_id))
            .count()
            .get_result::<i64>(&mut conn)
            .await
            .map_err(kassi_db::DbError::from)?
            > 0;

        if !exists {
            let mcfg_id = EntityId::new(EntityPrefix::MerchantConfig).to_string();
            let sig_id = EntityId::new(EntityPrefix::Signer).to_string();
            let webhook_secret = nanoid::nanoid!(32);

            conn.build_transaction()
                .run(|conn| {
                    let merchant_id = claims.merchant_id.clone();
                    let mcfg_id = mcfg_id.clone();
                    let sig_id = sig_id.clone();
                    let webhook_secret = webhook_secret.clone();
                    let address = claims.signer_address.clone();
                    let signer_type = claims.signer_type.clone();

                    Box::pin(async move {
                        kassi_db::diesel::insert_into(schema::merchants::table)
                            .values(kassi_db::models::NewMerchant { id: &merchant_id })
                            .execute(conn)
                            .await?;

                        kassi_db::diesel::insert_into(schema::merchant_configs::table)
                            .values(kassi_db::models::NewMerchantConfig {
                                id: &mcfg_id,
                                merchant_id: &merchant_id,
                                webhook_secret: &webhook_secret,
                            })
                            .execute(conn)
                            .await?;

                        kassi_db::diesel::insert_into(schema::signers::table)
                            .values(kassi_db::models::NewSigner {
                                id: &sig_id,
                                merchant_id: &merchant_id,
                                address: &address,
                                signer_type: &signer_type,
                            })
                            .execute(conn)
                            .await?;

                        Ok::<_, kassi_db::diesel::result::Error>(())
                    })
                })
                .await
                .map_err(kassi_db::DbError::from)?;
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

        let config = schema::merchant_configs::table
            .filter(schema::merchant_configs::api_key_hash.eq(&hash))
            .select(MerchantConfig::as_select())
            .first::<MerchantConfig>(&mut conn)
            .await
            .optional()
            .map_err(kassi_db::DbError::from)?
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
