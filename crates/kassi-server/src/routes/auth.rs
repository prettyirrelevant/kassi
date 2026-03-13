use alloy::hex;
use alloy::primitives::Address;
use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{Duration, Utc};
use ed25519_dalek::Verifier;
use jsonwebtoken::{encode, EncodingKey, Header};
use kassi_db::diesel_async::RunQueryDsl;
use kassi_db::models::NewNonce;
use kassi_db::queries::CreateMerchantParams;
use kassi_db::schema;
use kassi_types::{EntityId, EntityPrefix};
use serde::{Deserialize, Serialize};

use crate::errors::ServerError;
use crate::extractors::SessionAuth;
use crate::response::ApiSuccess;
use crate::AppState;

const NONCE_TTL_MINUTES: i64 = 5;
const JWT_EXPIRY_DAYS: i64 = 7;

#[derive(Serialize)]
struct NonceResponse {
    nonce: String,
}

#[derive(Deserialize)]
struct VerifyRequest {
    message: String,
    signature: String,
}

#[derive(Serialize)]
struct VerifyResponse {
    token: String,
    merchant_id: String,
}

#[derive(Deserialize)]
struct LinkRequest {
    message: String,
    signature: String,
}

#[derive(Serialize)]
struct LinkResponse {
    signer_id: String,
    address: String,
    signer_type: String,
}

#[derive(Serialize, Deserialize)]
pub struct Claims {
    pub merchant_id: String,
    pub signer_address: String,
    pub signer_type: String,
    pub exp: i64,
    pub iat: i64,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/nonce", get(get_nonce))
        .route("/auth/verify", post(verify))
        .route("/auth/link", post(link))
}

async fn get_nonce(
    State(state): State<AppState>,
) -> Result<ApiSuccess<NonceResponse>, ServerError> {
    let nonce = nanoid::nanoid!();
    let mut conn = state
        .db
        .get()
        .await
        .map_err(|e| kassi_db::DbError::Pool(e.to_string()))?;

    kassi_db::diesel::insert_into(schema::nonces::table)
        .values(NewNonce {
            nonce: &nonce,
            expires_at: Utc::now() + Duration::minutes(NONCE_TTL_MINUTES),
        })
        .execute(&mut conn)
        .await
        .map_err(kassi_db::DbError::from)?;

    Ok(ApiSuccess {
        data: NonceResponse { nonce },
    })
}

async fn verify(
    State(state): State<AppState>,
    Json(body): Json<VerifyRequest>,
) -> Result<ApiSuccess<VerifyResponse>, ServerError> {
    let (address, nonce, signer_type) = if body.message.contains("Ethereum account:") {
        verify_evm(&body.message, &body.signature)?
    } else if body.message.contains("Solana account:") {
        verify_solana(&body.message, &body.signature)?
    } else {
        return Err(ServerError::BadRequest("unsupported message format".into()));
    };

    let mut conn = state
        .db
        .get()
        .await
        .map_err(|e| kassi_db::DbError::Pool(e.to_string()))?;

    if !kassi_db::queries::consume_nonce(&mut conn, &nonce).await? {
        return Err(ServerError::AuthenticationRequired);
    }

    let merchant_id = if let Some(signer) =
        kassi_db::queries::find_signer_by_address(&mut conn, &address).await?
    {
        signer.merchant_id
    } else {
        let mer_id = EntityId::new(EntityPrefix::Merchant).to_string();

        let encrypted_seed = if let Some(kms) = &state.kms {
            Some(
                kassi_signer::create_merchant_seed(kms, &mer_id)
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
                merchant_id: &mer_id,
                config_id: EntityId::new(EntityPrefix::MerchantConfig).as_ref(),
                signer_id: EntityId::new(EntityPrefix::Signer).as_ref(),
                encrypted_seed: encrypted_seed.as_deref(),
                webhook_secret: &nanoid::nanoid!(32),
                signer_address: &address,
                signer_type: &signer_type,
            },
        )
        .await?;

        mer_id
    };

    let now = Utc::now();
    let token = encode(
        &Header::default(),
        &Claims {
            merchant_id: merchant_id.clone(),
            signer_address: address,
            signer_type,
            iat: now.timestamp(),
            exp: (now + Duration::days(JWT_EXPIRY_DAYS)).timestamp(),
        },
        &EncodingKey::from_secret(state.config.session_jwt_secret.as_bytes()),
    )
    .map_err(|e| ServerError::BadRequest(format!("failed to create token: {e}")))?;

    Ok(ApiSuccess {
        data: VerifyResponse { token, merchant_id },
    })
}

async fn link(
    State(state): State<AppState>,
    session: SessionAuth,
    Json(body): Json<LinkRequest>,
) -> Result<ApiSuccess<LinkResponse>, ServerError> {
    let (address, nonce, signer_type) = if body.message.contains("Ethereum account:") {
        verify_evm(&body.message, &body.signature)?
    } else if body.message.contains("Solana account:") {
        verify_solana(&body.message, &body.signature)?
    } else {
        return Err(ServerError::BadRequest("unsupported message format".into()));
    };

    let mut conn = state
        .db
        .get()
        .await
        .map_err(|e| kassi_db::DbError::Pool(e.to_string()))?;

    if !kassi_db::queries::consume_nonce(&mut conn, &nonce).await? {
        return Err(ServerError::AuthenticationRequired);
    }

    if kassi_db::queries::find_signer_by_address(&mut conn, &address)
        .await?
        .is_some()
    {
        return Err(ServerError::Conflict(
            "wallet is already linked to an account".into(),
        ));
    }

    let sig_id = EntityId::new(EntityPrefix::Signer).to_string();

    kassi_db::diesel::insert_into(schema::signers::table)
        .values(kassi_db::models::NewSigner {
            id: &sig_id,
            merchant_id: &session.merchant_id,
            address: &address,
            signer_type: &signer_type,
        })
        .execute(&mut conn)
        .await
        .map_err(kassi_db::DbError::from)?;

    Ok(ApiSuccess {
        data: LinkResponse {
            signer_id: sig_id,
            address,
            signer_type,
        },
    })
}

fn verify_evm(message: &str, signature: &str) -> Result<(String, String, String), ServerError> {
    let expected: Address = message
        .lines()
        .nth(1)
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .ok_or_else(|| ServerError::BadRequest("missing address in SIWE message".into()))?
        .parse()
        .map_err(|_| ServerError::BadRequest("invalid Ethereum address in message".into()))?;

    let nonce = message
        .lines()
        .find_map(|l| l.strip_prefix("Nonce: "))
        .ok_or_else(|| ServerError::BadRequest("missing nonce in SIWE message".into()))?;

    let sig_hex = signature.strip_prefix("0x").unwrap_or(signature);
    let sig_bytes = hex::decode(sig_hex)
        .map_err(|_| ServerError::BadRequest("invalid signature hex encoding".into()))?;

    let sig = alloy::primitives::Signature::try_from(sig_bytes.as_slice())
        .map_err(|_| ServerError::AuthenticationRequired)?;

    let recovered = sig
        .recover_address_from_msg(message)
        .map_err(|_| ServerError::AuthenticationRequired)?;

    if recovered != expected {
        return Err(ServerError::AuthenticationRequired);
    }

    Ok((expected.to_checksum(None), nonce.to_string(), "evm".into()))
}

fn verify_solana(message: &str, signature: &str) -> Result<(String, String, String), ServerError> {
    let address = message
        .lines()
        .skip_while(|l| !l.contains("Solana account:"))
        .nth(1)
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .ok_or_else(|| ServerError::BadRequest("missing Solana address in message".into()))?;

    let nonce = message
        .lines()
        .find_map(|l| l.strip_prefix("Nonce: "))
        .ok_or_else(|| ServerError::BadRequest("missing nonce in message".into()))?;

    let pubkey_bytes: [u8; 32] = bs58::decode(address)
        .into_vec()
        .map_err(|_| ServerError::BadRequest("invalid Solana address".into()))?
        .try_into()
        .map_err(|_| ServerError::BadRequest("invalid Solana address length".into()))?;

    let sig_bytes: [u8; 64] = bs58::decode(signature)
        .into_vec()
        .map_err(|_| ServerError::BadRequest("invalid signature encoding".into()))?
        .try_into()
        .map_err(|_| ServerError::BadRequest("invalid signature length".into()))?;

    let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(&pubkey_bytes)
        .map_err(|_| ServerError::BadRequest("invalid Solana public key".into()))?;

    verifying_key
        .verify(
            message.as_bytes(),
            &ed25519_dalek::Signature::from_bytes(&sig_bytes),
        )
        .map_err(|_| ServerError::AuthenticationRequired)?;

    Ok((address.to_string(), nonce.to_string(), "solana".into()))
}
