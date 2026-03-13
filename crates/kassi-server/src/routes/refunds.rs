use std::collections::HashMap;
use std::str::FromStr;

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{DateTime, Utc};
use kassi_db::diesel::prelude::*;
use kassi_db::diesel_async::RunQueryDsl;
use kassi_db::models::{
    Asset, DepositAddress, LedgerEntry, Network, NetworkAddress, NewJob, NewLedgerEntry,
};
use kassi_db::schema;
use kassi_types::{EntityId, EntityPrefix};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::errors::{ServerError, ValidationDetail};
use crate::extractors::AnyAuth;
use crate::response::{ApiList, ApiSuccess, ListMeta};
use crate::AppState;

// -- request types --

#[derive(Deserialize)]
struct RefundRequest {
    amount: Option<String>,
    destination: Option<String>,
    reason: Option<String>,
}

#[derive(Deserialize)]
struct ListParams {
    page: Option<String>,
    limit: Option<usize>,
}

// -- response types --

#[derive(Serialize)]
struct NetworkEmbed {
    id: String,
    display_name: String,
}

impl From<&Network> for NetworkEmbed {
    fn from(n: &Network) -> Self {
        Self {
            id: n.id.clone(),
            display_name: n.display_name.clone(),
        }
    }
}

#[derive(Serialize)]
struct AssetEmbed {
    id: String,
    symbol: String,
    decimals: i32,
}

impl From<&Asset> for AssetEmbed {
    fn from(a: &Asset) -> Self {
        Self {
            id: a.id.clone(),
            symbol: a.symbol.clone(),
            decimals: a.decimals,
        }
    }
}

#[derive(Serialize)]
struct DepositAddressEmbed {
    id: String,
    address: String,
}

#[derive(Serialize)]
struct RefundResponse {
    id: String,
    deposit_address: DepositAddressEmbed,
    payment_intent_id: Option<String>,
    asset: AssetEmbed,
    network: NetworkEmbed,
    entry_type: String,
    status: String,
    amount: String,
    destination: Option<String>,
    onchain_ref: String,
    reason: Option<String>,
    created_at: DateTime<Utc>,
}

// -- helpers --

fn encode_cursor(created_at: &DateTime<Utc>, id: &str) -> String {
    URL_SAFE_NO_PAD.encode(format!("{}|{}", created_at.to_rfc3339(), id))
}

fn decode_cursor(cursor: &str) -> Result<(DateTime<Utc>, String), ServerError> {
    let raw = String::from_utf8(
        URL_SAFE_NO_PAD
            .decode(cursor)
            .map_err(|_| ServerError::BadRequest("invalid pagination cursor".into()))?,
    )
    .map_err(|_| ServerError::BadRequest("invalid pagination cursor".into()))?;

    let (time_str, id) = raw
        .split_once('|')
        .ok_or_else(|| ServerError::BadRequest("invalid pagination cursor".into()))?;

    Ok((
        time_str
            .parse()
            .map_err(|_| ServerError::BadRequest("invalid pagination cursor".into()))?,
        id.to_string(),
    ))
}

fn validate_refund_request(body: &RefundRequest) -> Result<(&str, &str), ServerError> {
    let mut errors = Vec::new();

    let amount = match &body.amount {
        Some(v) if !v.is_empty() => v.as_str(),
        _ => {
            errors.push(ValidationDetail {
                field: "amount".into(),
                code: "required",
                message: "this field is required.".into(),
            });
            ""
        }
    };

    let destination = match &body.destination {
        Some(v) if !v.is_empty() => v.as_str(),
        _ => {
            errors.push(ValidationDetail {
                field: "destination".into(),
                code: "required",
                message: "this field is required.".into(),
            });
            ""
        }
    };

    // validate amount is a positive integer (smallest unit)
    if !amount.is_empty() {
        match Decimal::from_str(amount) {
            Ok(d) if d > Decimal::ZERO && d == d.floor() => {}
            _ => {
                errors.push(ValidationDetail {
                    field: "amount".into(),
                    code: "invalid_field_value",
                    message: "must be a positive integer string (smallest unit).".into(),
                });
            }
        }
    }

    if !errors.is_empty() {
        return Err(ServerError::ValidationFailed(errors));
    }

    Ok((amount, destination))
}

/// Create a refund ledger entry and enqueue a refund job.
/// Shared logic for both payment intent and deposit address refunds.
async fn create_refund(
    state: &AppState,
    deposit_address: &DepositAddress,
    payment_intent_id: Option<&str>,
    asset_id: &str,
    network_id: &str,
    amount: &str,
    destination: &str,
    reason: Option<&str>,
) -> Result<(LedgerEntry, kassi_db::models::Job), ServerError> {
    let mut conn = state
        .db
        .get()
        .await
        .map_err(|e| kassi_db::DbError::Pool(e.to_string()))?;

    // generate a placeholder onchain_ref (the actual tx hash comes from the worker)
    let le_id = EntityId::new(EntityPrefix::LedgerEntry).to_string();
    let onchain_ref = format!("pending:{le_id}");

    let ledger_entry = kassi_db::queries::insert_ledger_entry(
        &mut conn,
        NewLedgerEntry {
            id: &le_id,
            deposit_address_id: &deposit_address.id,
            payment_intent_id,
            asset_id,
            network_id,
            entry_type: "refund",
            status: "pending",
            amount,
            destination: Some(destination),
            onchain_ref: &onchain_ref,
            reason,
        },
    )
    .await?;

    let job_payload = serde_json::json!({
        "ledger_entry_id": le_id,
        "deposit_address_id": deposit_address.id,
        "payment_intent_id": payment_intent_id,
        "asset_id": asset_id,
        "network_id": network_id,
        "amount": amount,
        "destination": destination,
        "reason": reason,
    });

    let job = kassi_db::queries::insert_job(
        &mut conn,
        NewJob {
            queue: "refunds",
            payload: job_payload,
            max_attempts: 5,
            scheduled_at: None,
        },
    )
    .await?;

    Ok((ledger_entry, job))
}

// -- routes --

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/payment-intents/{id}/refund",
            post(refund_payment_intent),
        )
        .route(
            "/deposit-addresses/{id}/refund",
            post(refund_deposit_address),
        )
        .route("/refunds", get(list_refunds))
}

async fn refund_payment_intent(
    State(state): State<AppState>,
    auth: AnyAuth,
    Path(id): Path<String>,
    Json(body): Json<RefundRequest>,
) -> Result<axum::response::Response, ServerError> {
    let (amount, destination) = validate_refund_request(&body)?;

    let mut conn = state
        .db
        .get()
        .await
        .map_err(|e| kassi_db::DbError::Pool(e.to_string()))?;

    // load payment intent scoped to merchant
    let pi = kassi_db::queries::get_payment_intent(&mut conn, &id, &auth.merchant_id)
        .await?
        .ok_or_else(|| ServerError::NotFound {
            entity: "payment_intent",
            id: id.clone(),
        })?;

    // only confirmed intents can be refunded
    if pi.status != "confirmed" {
        return Err(ServerError::Conflict(format!(
            "payment intent '{id}' has status '{}'; only confirmed intents can be refunded.",
            pi.status
        )));
    }

    // resolve the asset and network from the intent's quote
    let quotes = kassi_db::queries::load_quotes_by_payment_intent_ids(
        &mut conn,
        &[pi.id.as_str()],
    )
    .await?;
    let quote = quotes.into_iter().next().ok_or_else(|| {
        ServerError::BadRequest(format!("no quote found for payment intent '{id}'."))
    })?;

    let asset = kassi_db::queries::get_asset_by_id(&mut conn, &quote.asset_id)
        .await?
        .ok_or_else(|| {
            ServerError::BadRequest(format!("asset '{}' not found.", quote.asset_id))
        })?;

    // load deposit address (unscoped since we already verified ownership via the PI)
    let deposit_address = kassi_db::queries::get_deposit_address_unscoped(
        &mut conn,
        &pi.deposit_address_id,
    )
    .await?
    .ok_or_else(|| ServerError::BadRequest("deposit address not found.".into()))?;

    drop(conn);

    let (ledger_entry, _job) = create_refund(
        &state,
        &deposit_address,
        Some(&pi.id),
        &asset.id,
        &asset.network_id,
        amount,
        destination,
        body.reason.as_deref(),
    )
    .await?;

    // build response
    let mut conn = state
        .db
        .get()
        .await
        .map_err(|e| kassi_db::DbError::Pool(e.to_string()))?;

    let network = kassi_db::queries::load_networks_by_ids(&mut conn, &[asset.network_id.as_str()])
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| ServerError::BadRequest("network not found.".into()))?;

    let dep_address_str =
        kassi_db::queries::first_network_address_string(&mut conn, &deposit_address.id)
            .await?
            .unwrap_or_default();

    let response = RefundResponse {
        id: ledger_entry.id,
        deposit_address: DepositAddressEmbed {
            id: deposit_address.id,
            address: dep_address_str,
        },
        payment_intent_id: ledger_entry.payment_intent_id,
        asset: AssetEmbed::from(&asset),
        network: NetworkEmbed::from(&network),
        entry_type: ledger_entry.entry_type,
        status: ledger_entry.status,
        amount: ledger_entry.amount,
        destination: ledger_entry.destination,
        onchain_ref: ledger_entry.onchain_ref,
        reason: ledger_entry.reason,
        created_at: ledger_entry.created_at,
    };

    Ok(ApiSuccess::created(response))
}

async fn refund_deposit_address(
    State(state): State<AppState>,
    auth: AnyAuth,
    Path(id): Path<String>,
    Json(body): Json<RefundRequest>,
) -> Result<axum::response::Response, ServerError> {
    let (amount, destination) = validate_refund_request(&body)?;

    let mut conn = state
        .db
        .get()
        .await
        .map_err(|e| kassi_db::DbError::Pool(e.to_string()))?;

    // load deposit address scoped to merchant
    let deposit_address = kassi_db::queries::get_deposit_address(&mut conn, &id, &auth.merchant_id)
        .await?
        .ok_or_else(|| ServerError::NotFound {
            entity: "deposit_address",
            id: id.clone(),
        })?;

    // for deposit address refunds, we need at least one confirmed deposit
    let has_confirmed_deposit = schema::ledger_entries::table
        .filter(schema::ledger_entries::deposit_address_id.eq(&deposit_address.id))
        .filter(schema::ledger_entries::entry_type.eq("deposit"))
        .filter(schema::ledger_entries::status.eq("confirmed"))
        .count()
        .get_result::<i64>(&mut conn)
        .await
        .map_err(kassi_db::DbError::from)?
        > 0;

    if !has_confirmed_deposit {
        return Err(ServerError::Conflict(
            "no confirmed deposits found for this address; cannot refund.".into(),
        ));
    }

    // resolve asset and network from a confirmed deposit entry
    let deposit_entry = schema::ledger_entries::table
        .filter(schema::ledger_entries::deposit_address_id.eq(&deposit_address.id))
        .filter(schema::ledger_entries::entry_type.eq("deposit"))
        .filter(schema::ledger_entries::status.eq("confirmed"))
        .order(schema::ledger_entries::created_at.desc())
        .select(kassi_db::models::LedgerEntry::as_select())
        .first::<kassi_db::models::LedgerEntry>(&mut conn)
        .await
        .map_err(kassi_db::DbError::from)?;

    drop(conn);

    let (ledger_entry, _job) = create_refund(
        &state,
        &deposit_address,
        None,
        &deposit_entry.asset_id,
        &deposit_entry.network_id,
        amount,
        destination,
        body.reason.as_deref(),
    )
    .await?;

    // build response
    let mut conn = state
        .db
        .get()
        .await
        .map_err(|e| kassi_db::DbError::Pool(e.to_string()))?;

    let assets =
        kassi_db::queries::load_assets_by_ids(&mut conn, &[deposit_entry.asset_id.as_str()])
            .await?;
    let asset = assets.into_iter().next().ok_or_else(|| {
        ServerError::BadRequest("asset not found.".into())
    })?;

    let networks =
        kassi_db::queries::load_networks_by_ids(&mut conn, &[deposit_entry.network_id.as_str()])
            .await?;
    let network = networks.into_iter().next().ok_or_else(|| {
        ServerError::BadRequest("network not found.".into())
    })?;

    let dep_address_str =
        kassi_db::queries::first_network_address_string(&mut conn, &deposit_address.id)
            .await?
            .unwrap_or_default();

    let response = RefundResponse {
        id: ledger_entry.id,
        deposit_address: DepositAddressEmbed {
            id: deposit_address.id,
            address: dep_address_str,
        },
        payment_intent_id: ledger_entry.payment_intent_id,
        asset: AssetEmbed::from(&asset),
        network: NetworkEmbed::from(&network),
        entry_type: ledger_entry.entry_type,
        status: ledger_entry.status,
        amount: ledger_entry.amount,
        destination: ledger_entry.destination,
        onchain_ref: ledger_entry.onchain_ref,
        reason: ledger_entry.reason,
        created_at: ledger_entry.created_at,
    };

    Ok(ApiSuccess::created(response))
}

async fn list_refunds(
    State(state): State<AppState>,
    auth: AnyAuth,
    Query(params): Query<ListParams>,
) -> Result<ApiList<RefundResponse>, ServerError> {
    let limit = params.limit.unwrap_or(20).min(100);

    let mut conn = state
        .db
        .get()
        .await
        .map_err(|e| kassi_db::DbError::Pool(e.to_string()))?;

    let cursor = params.page.as_ref().map(|p| decode_cursor(p)).transpose()?;
    let cursor_ref = cursor
        .as_ref()
        .map(|(t, id)| (t, id.as_str()));

    let mut rows = kassi_db::queries::list_refund_ledger_entries(
        &mut conn,
        &auth.merchant_id,
        i64::try_from(limit + 1).unwrap_or(i64::MAX),
        cursor_ref,
    )
    .await?;

    let has_next = rows.len() > limit;
    if has_next {
        rows.truncate(limit);
    }

    let next_page = if has_next {
        rows.last()
            .map(|(le, _)| encode_cursor(&le.created_at, &le.id))
    } else {
        None
    };

    // load assets and networks for all entries
    let asset_ids: Vec<&str> = rows.iter().map(|(le, _)| le.asset_id.as_str()).collect();
    let network_ids: Vec<&str> = rows.iter().map(|(le, _)| le.network_id.as_str()).collect();

    let assets = kassi_db::queries::load_assets_by_ids(&mut conn, &asset_ids).await?;
    let networks = kassi_db::queries::load_networks_by_ids(&mut conn, &network_ids).await?;

    let asset_map: HashMap<&str, &Asset> = assets.iter().map(|a| (a.id.as_str(), a)).collect();
    let network_map: HashMap<&str, &Network> =
        networks.iter().map(|n| (n.id.as_str(), n)).collect();

    // load first network address for each deposit address (for embed)
    let dep_ids: Vec<&str> = rows.iter().map(|(_, da)| da.id.as_str()).collect();
    let network_addresses: Vec<NetworkAddress> = if dep_ids.is_empty() {
        vec![]
    } else {
        schema::network_addresses::table
            .filter(schema::network_addresses::deposit_address_id.eq_any(&dep_ids))
            .select(NetworkAddress::as_select())
            .load::<NetworkAddress>(&mut conn)
            .await
            .map_err(kassi_db::DbError::from)?
    };
    let mut na_map: HashMap<&str, &str> = HashMap::new();
    for na in &network_addresses {
        na_map
            .entry(na.deposit_address_id.as_str())
            .or_insert(na.address.as_str());
    }

    let data: Vec<RefundResponse> = rows
        .iter()
        .filter_map(|(le, da)| {
            let asset = asset_map.get(le.asset_id.as_str())?;
            let network = network_map.get(le.network_id.as_str())?;
            let addr = na_map.get(da.id.as_str()).copied().unwrap_or("");

            Some(RefundResponse {
                id: le.id.clone(),
                deposit_address: DepositAddressEmbed {
                    id: da.id.clone(),
                    address: addr.to_string(),
                },
                payment_intent_id: le.payment_intent_id.clone(),
                asset: AssetEmbed::from(*asset),
                network: NetworkEmbed::from(*network),
                entry_type: le.entry_type.clone(),
                status: le.status.clone(),
                amount: le.amount.clone(),
                destination: le.destination.clone(),
                onchain_ref: le.onchain_ref.clone(),
                reason: le.reason.clone(),
                created_at: le.created_at,
            })
        })
        .collect();

    Ok(ApiList {
        data,
        meta: ListMeta {
            next_page,
            previous_page: None,
        },
    })
}
