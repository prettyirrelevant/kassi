use alloy::hex;
use axum::extract::{Path, State};
use axum::routing::{delete, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use ed25519_dalek::Verifier;
use kassi_db::diesel::prelude::*;
use kassi_db::diesel_async::RunQueryDsl;
use kassi_db::models::{NewSettlementDestination, SettlementDestination, Signer};
use kassi_db::schema;
use kassi_types::{Caip2, EntityId, EntityPrefix};
use serde::{Deserialize, Serialize};

use crate::errors::{ServerError, ValidationDetail};

use crate::extractors::{AnyAuth, SessionAuth};
use crate::response::{ApiList, ApiSuccess, ListMeta};
use crate::AppState;

#[derive(Serialize)]
struct SettlementDestinationResponse {
    id: String,
    merchant_id: String,
    network_id: String,
    address: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct CreateRequest {
    network_ids: Vec<String>,
    address: String,
    signature: String,
}

#[derive(Deserialize)]
struct DeleteRequest {
    signature: String,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/settlement-destinations",
            post(create_settlement_destination).get(list_settlement_destinations),
        )
        .route(
            "/settlement-destinations/{id}",
            delete(delete_settlement_destination),
        )
}

fn confirmation_message_create(address: &str, network_ids: &[String]) -> String {
    let networks = network_ids.join(", ");
    format!("I confirm setting {address} as the settlement destination for networks: {networks}")
}

fn confirmation_message_delete(id: &str) -> String {
    format!("I confirm removing settlement destination {id}")
}

fn verify_evm_signature(
    message: &str,
    signature: &str,
    expected_address: &str,
) -> Result<(), ServerError> {
    let sig_hex = signature.strip_prefix("0x").unwrap_or(signature);
    let sig_bytes = hex::decode(sig_hex).map_err(|_| ServerError::InvalidSignature)?;

    if sig_bytes.len() != 65 {
        return Err(ServerError::InvalidSignature);
    }

    let sig = alloy::primitives::Signature::try_from(sig_bytes.as_slice())
        .map_err(|_| ServerError::InvalidSignature)?;

    let recovered = sig
        .recover_address_from_msg(message)
        .map_err(|_| ServerError::InvalidSignature)?;

    let expected: alloy::primitives::Address = expected_address
        .parse()
        .map_err(|_| ServerError::InvalidSignature)?;

    if recovered != expected {
        return Err(ServerError::InvalidSignature);
    }

    Ok(())
}

fn verify_solana_signature(
    message: &str,
    signature: &str,
    expected_address: &str,
) -> Result<(), ServerError> {
    let pubkey_bytes: [u8; 32] = bs58::decode(expected_address)
        .into_vec()
        .map_err(|_| ServerError::InvalidSignature)?
        .try_into()
        .map_err(|_| ServerError::InvalidSignature)?;

    let sig_bytes: [u8; 64] = bs58::decode(signature)
        .into_vec()
        .map_err(|_| ServerError::InvalidSignature)?
        .try_into()
        .map_err(|_| ServerError::InvalidSignature)?;

    let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(&pubkey_bytes)
        .map_err(|_| ServerError::InvalidSignature)?;

    verifying_key
        .verify(
            message.as_bytes(),
            &ed25519_dalek::Signature::from_bytes(&sig_bytes),
        )
        .map_err(|_| ServerError::InvalidSignature)?;

    Ok(())
}

fn verify_confirmation_signature(
    message: &str,
    signature: &str,
    signer: &Signer,
) -> Result<(), ServerError> {
    match signer.signer_type.as_str() {
        "evm" => verify_evm_signature(message, signature, &signer.address),
        "solana" => verify_solana_signature(message, signature, &signer.address),
        _ => Err(ServerError::BadRequest("unsupported signer type".into())),
    }
}

async fn create_settlement_destination(
    State(state): State<AppState>,
    session: SessionAuth,
    Json(body): Json<CreateRequest>,
) -> Result<axum::response::Response, ServerError> {
    if body.network_ids.is_empty() {
        return Err(ServerError::ValidationFailed(vec![ValidationDetail {
            field: "network_ids".into(),
            code: "required",
            message: "at least one network id is required.".into(),
        }]));
    }

    if body.address.trim().is_empty() {
        return Err(ServerError::ValidationFailed(vec![ValidationDetail {
            field: "address".into(),
            code: "required",
            message: "address is required.".into(),
        }]));
    }

    let caip2_ids: Vec<Caip2> = body
        .network_ids
        .iter()
        .map(|id| {
            id.parse::<Caip2>().map_err(|_| {
                ServerError::ValidationFailed(vec![ValidationDetail {
                    field: "network_ids".into(),
                    code: "invalid_caip_identifier",
                    message: format!("'{id}' is not a valid CAIP-2 identifier."),
                }])
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let first_ns = caip2_ids[0].namespace();
    if !caip2_ids.iter().all(|id| id.namespace() == first_ns) {
        return Err(ServerError::ValidationFailed(vec![ValidationDetail {
            field: "network_ids".into(),
            code: "invalid_field_value",
            message: "all network ids must share the same namespace.".into(),
        }]));
    }

    let mut conn = state
        .db
        .get()
        .await
        .map_err(|e| kassi_db::DbError::Pool(e.to_string()))?;

    let existing_count: i64 = schema::networks::table
        .filter(schema::networks::id.eq_any(&body.network_ids))
        .count()
        .get_result(&mut conn)
        .await
        .map_err(kassi_db::DbError::from)?;

    if existing_count != body.network_ids.len() as i64 {
        return Err(ServerError::ValidationFailed(vec![ValidationDetail {
            field: "network_ids".into(),
            code: "invalid_field_value",
            message: "one or more network ids do not exist.".into(),
        }]));
    }

    let signer = schema::signers::table
        .filter(schema::signers::merchant_id.eq(&session.merchant_id))
        .select(Signer::as_select())
        .first::<Signer>(&mut conn)
        .await
        .optional()
        .map_err(kassi_db::DbError::from)?
        .ok_or_else(|| ServerError::BadRequest("no signer found for this merchant.".into()))?;

    let message = confirmation_message_create(&body.address, &body.network_ids);
    verify_confirmation_signature(&message, &body.signature, &signer)?;

    let mut results: Vec<SettlementDestinationResponse> = Vec::new();

    for network_id in &body.network_ids {
        let id = EntityId::new(EntityPrefix::SettlementDestination).to_string();
        let now = Utc::now();

        let dest = kassi_db::diesel::insert_into(schema::settlement_destinations::table)
            .values(NewSettlementDestination {
                id: &id,
                merchant_id: &session.merchant_id,
                network_id,
                address: &body.address,
            })
            .on_conflict((
                schema::settlement_destinations::merchant_id,
                schema::settlement_destinations::network_id,
            ))
            .do_update()
            .set((
                schema::settlement_destinations::address.eq(&body.address),
                schema::settlement_destinations::updated_at.eq(now),
            ))
            .returning(SettlementDestination::as_returning())
            .get_result::<SettlementDestination>(&mut conn)
            .await
            .map_err(kassi_db::DbError::from)?;

        results.push(SettlementDestinationResponse {
            id: dest.id,
            merchant_id: dest.merchant_id,
            network_id: dest.network_id,
            address: dest.address,
            created_at: dest.created_at,
            updated_at: dest.updated_at,
        });
    }

    Ok(ApiSuccess::created(results))
}

async fn list_settlement_destinations(
    State(state): State<AppState>,
    auth: AnyAuth,
) -> Result<ApiList<SettlementDestinationResponse>, ServerError> {
    let mut conn = state
        .db
        .get()
        .await
        .map_err(|e| kassi_db::DbError::Pool(e.to_string()))?;

    let destinations = schema::settlement_destinations::table
        .filter(schema::settlement_destinations::merchant_id.eq(&auth.merchant_id))
        .select(SettlementDestination::as_select())
        .order(schema::settlement_destinations::created_at.desc())
        .load::<SettlementDestination>(&mut conn)
        .await
        .map_err(kassi_db::DbError::from)?;

    let data = destinations
        .into_iter()
        .map(|d| SettlementDestinationResponse {
            id: d.id,
            merchant_id: d.merchant_id,
            network_id: d.network_id,
            address: d.address,
            created_at: d.created_at,
            updated_at: d.updated_at,
        })
        .collect();

    Ok(ApiList {
        data,
        meta: ListMeta {
            next_page: None,
            previous_page: None,
        },
    })
}

async fn delete_settlement_destination(
    State(state): State<AppState>,
    session: SessionAuth,
    Path(id): Path<String>,
    Json(body): Json<DeleteRequest>,
) -> Result<axum::http::StatusCode, ServerError> {
    let mut conn = state
        .db
        .get()
        .await
        .map_err(|e| kassi_db::DbError::Pool(e.to_string()))?;

    let dest = schema::settlement_destinations::table
        .filter(schema::settlement_destinations::id.eq(&id))
        .filter(schema::settlement_destinations::merchant_id.eq(&session.merchant_id))
        .select(SettlementDestination::as_select())
        .first::<SettlementDestination>(&mut conn)
        .await
        .optional()
        .map_err(kassi_db::DbError::from)?
        .ok_or_else(|| ServerError::NotFound {
            entity: "settlement_destination",
            id: id.clone(),
        })?;

    let signer = schema::signers::table
        .filter(schema::signers::merchant_id.eq(&session.merchant_id))
        .select(Signer::as_select())
        .first::<Signer>(&mut conn)
        .await
        .optional()
        .map_err(kassi_db::DbError::from)?
        .ok_or_else(|| ServerError::BadRequest("no signer found for this merchant.".into()))?;

    let message = confirmation_message_delete(&dest.id);
    verify_confirmation_signature(&message, &body.signature, &signer)?;

    kassi_db::diesel::delete(
        schema::settlement_destinations::table
            .filter(schema::settlement_destinations::id.eq(&dest.id)),
    )
    .execute(&mut conn)
    .await
    .map_err(kassi_db::DbError::from)?;

    Ok(axum::http::StatusCode::NO_CONTENT)
}
