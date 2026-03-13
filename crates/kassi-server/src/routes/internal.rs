use std::collections::HashMap;

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use kassi_db::models::NewJob;
use serde::{Deserialize, Serialize};

use crate::errors::{ServerError, ValidationDetail};
use crate::extractors::InternalAuth;
use crate::response::ApiSuccess;
use crate::AppState;

#[derive(Deserialize)]
struct DepositRequest {
    network_id: Option<String>,
    tx_hash: Option<String>,
    from_address: Option<String>,
    to_address: Option<String>,
    amount: Option<String>,
    token_address: Option<String>,
    block_number: Option<i64>,
}

#[derive(Serialize)]
struct DepositResponse {
    status: &'static str,
}

fn require_str<'a>(
    field: Option<&'a str>,
    name: &'static str,
    errors: &mut Vec<ValidationDetail>,
) -> &'a str {
    match field {
        Some(v) if !v.is_empty() => v,
        _ => {
            errors.push(ValidationDetail {
                field: name.into(),
                code: "required",
                message: "this field is required.".into(),
            });
            ""
        }
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/internal/deposits", post(create_deposit))
        .route("/internal/addresses", get(list_addresses))
}

async fn create_deposit(
    State(state): State<AppState>,
    _auth: InternalAuth,
    Json(body): Json<DepositRequest>,
) -> Result<ApiSuccess<DepositResponse>, ServerError> {
    let mut errors = Vec::new();

    let network_id = require_str(body.network_id.as_deref(), "network_id", &mut errors);
    let tx_hash = require_str(body.tx_hash.as_deref(), "tx_hash", &mut errors);
    let from_address = require_str(body.from_address.as_deref(), "from_address", &mut errors);
    let to_address = require_str(body.to_address.as_deref(), "to_address", &mut errors);
    let amount = require_str(body.amount.as_deref(), "amount", &mut errors);
    let token_address = require_str(body.token_address.as_deref(), "token_address", &mut errors);

    let block_number = if let Some(n) = body.block_number {
        n
    } else {
        errors.push(ValidationDetail {
            field: "block_number".into(),
            code: "required",
            message: "this field is required.".into(),
        });
        0
    };

    if !errors.is_empty() {
        return Err(ServerError::ValidationFailed(errors));
    }

    let mut conn = state
        .db
        .get()
        .await
        .map_err(|e| kassi_db::DbError::Pool(e.to_string()))?;

    kassi_db::queries::find_network_address(&mut conn, to_address, network_id)
        .await?
        .ok_or_else(|| ServerError::NotFound {
            entity: "network_address",
            id: to_address.to_string(),
        })?;

    kassi_db::queries::insert_job(
        &mut conn,
        NewJob {
            queue: "deposits",
            payload: serde_json::json!({
                "network_id": network_id,
                "tx_hash": tx_hash,
                "from_address": from_address,
                "to_address": to_address,
                "amount": amount,
                "token_address": token_address,
                "block_number": block_number,
            }),
            max_attempts: 5,
            scheduled_at: None,
        },
    )
    .await?;

    Ok(ApiSuccess {
        data: DepositResponse { status: "accepted" },
    })
}

async fn list_addresses(
    State(state): State<AppState>,
    _auth: InternalAuth,
) -> Result<Json<HashMap<String, Vec<String>>>, ServerError> {
    let mut conn = state
        .db
        .get()
        .await
        .map_err(|e| kassi_db::DbError::Pool(e.to_string()))?;

    let rows = kassi_db::queries::all_network_addresses(&mut conn).await?;

    let mut grouped: HashMap<String, Vec<String>> = HashMap::new();
    for (network_id, address) in rows {
        grouped.entry(network_id).or_default().push(address);
    }

    Ok(Json(grouped))
}
