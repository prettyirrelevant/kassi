use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{DateTime, Utc};
use kassi_db::diesel::prelude::*;
use kassi_db::diesel_async::RunQueryDsl;
use kassi_db::models::{
    DepositAddress, LedgerEntry, Network, NetworkAddress, NewDepositAddress, NewNetworkAddress,
};
use kassi_db::schema;
use kassi_signer::Namespace;
use kassi_types::{EntityId, EntityPrefix};
use serde::{Deserialize, Serialize};

use crate::errors::{ServerError, ValidationDetail};
use crate::extractors::AnyAuth;
use crate::response::{ApiList, ApiSuccess, ListMeta};
use crate::AppState;

// -- response types --

#[derive(Serialize)]
struct NetworkEmbed {
    id: String,
    display_name: String,
    created_at: DateTime<Utc>,
}

impl From<&Network> for NetworkEmbed {
    fn from(n: &Network) -> Self {
        Self {
            id: n.id.clone(),
            display_name: n.display_name.clone(),
            created_at: n.created_at,
        }
    }
}

#[derive(Serialize)]
struct NetworkAddressResponse {
    id: String,
    network: NetworkEmbed,
    address: String,
}

#[derive(Serialize)]
struct DepositAddressResponse {
    id: String,
    merchant_id: String,
    label: Option<String>,
    address_type: String,
    network_addresses: Vec<NetworkAddressResponse>,
    created_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct AssetEmbed {
    id: String,
    symbol: String,
    decimals: i32,
}

#[derive(Serialize)]
struct DepositAddressEmbed {
    id: String,
    address: String,
}

#[derive(Serialize)]
struct LedgerEntryResponse {
    id: String,
    deposit_address: DepositAddressEmbed,
    payment_intent_id: Option<String>,
    asset: AssetEmbed,
    network: NetworkEmbed,
    entry_type: String,
    status: String,
    amount: String,
    fee_amount: Option<String>,
    sender: Option<String>,
    destination: Option<String>,
    onchain_ref: String,
    reason: Option<String>,
    created_at: DateTime<Utc>,
}

// -- request types --

#[derive(Deserialize)]
struct CreateRequest {
    label: Option<String>,
    address_type: Option<String>,
}

#[derive(Deserialize)]
struct PaginationParams {
    page: Option<String>,
    limit: Option<usize>,
}

// -- helpers --

fn parse_chain_id_for_derivation(network_id: &str, namespace: &str) -> Result<u64, ServerError> {
    match namespace {
        "eip155" => network_id
            .strip_prefix("eip155:")
            .ok_or_else(|| ServerError::BadRequest("invalid eip155 network id".into()))?
            .parse::<u64>()
            .map_err(|_| ServerError::BadRequest("invalid evm chain id".into())),
        "solana" => Ok(501),
        _ => Err(ServerError::BadRequest(format!(
            "unsupported namespace: {namespace}"
        ))),
    }
}

fn namespace_from_str(s: &str) -> Result<Namespace, ServerError> {
    match s {
        "eip155" => Ok(Namespace::Evm),
        "solana" => Ok(Namespace::Solana),
        _ => Err(ServerError::BadRequest(format!(
            "unsupported namespace: {s}"
        ))),
    }
}

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

fn network_address_response(na: NetworkAddress, net: &Network) -> NetworkAddressResponse {
    NetworkAddressResponse {
        id: na.id,
        network: NetworkEmbed::from(net),
        address: na.address,
    }
}

fn deposit_address_response(
    da: DepositAddress,
    network_addresses: Vec<NetworkAddressResponse>,
) -> DepositAddressResponse {
    DepositAddressResponse {
        id: da.id,
        merchant_id: da.merchant_id,
        label: da.label,
        address_type: da.address_type,
        network_addresses,
        created_at: da.created_at,
    }
}

/// Group loaded `(NetworkAddress, Network)` pairs into a map keyed by `deposit_address_id`.
fn group_network_addresses(
    rows: Vec<(NetworkAddress, Network)>,
) -> HashMap<String, Vec<NetworkAddressResponse>> {
    let mut grouped: HashMap<String, Vec<NetworkAddressResponse>> = HashMap::new();
    for (na, net) in rows {
        let dep_id = na.deposit_address_id.clone();
        grouped
            .entry(dep_id)
            .or_default()
            .push(network_address_response(na, &net));
    }
    grouped
}

// -- routes --

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/deposit-addresses",
            post(create_deposit_address).get(list_deposit_addresses),
        )
        .route("/deposit-addresses/{id}", get(get_deposit_address))
        .route(
            "/deposit-addresses/{id}/ledger-entries",
            get(list_ledger_entries),
        )
}

async fn create_deposit_address(
    State(state): State<AppState>,
    auth: AnyAuth,
    Json(body): Json<CreateRequest>,
) -> Result<axum::response::Response, ServerError> {
    let address_type = body.address_type.as_deref().unwrap_or("reusable");
    if address_type != "reusable" && address_type != "one_off" {
        return Err(ServerError::ValidationFailed(vec![ValidationDetail {
            field: "address_type".into(),
            code: "invalid_field_value",
            message: "must be 'reusable' or 'one_off'".into(),
        }]));
    }

    let kms = state
        .kms
        .as_ref()
        .ok_or_else(|| ServerError::BadRequest("key management service not configured".into()))?;

    let mut conn = state
        .db
        .get()
        .await
        .map_err(|e| kassi_db::DbError::Pool(e.to_string()))?;

    let encrypted_seed = kassi_db::queries::get_merchant_config(&mut conn, &auth.merchant_id)
        .await?
        .encrypted_seed
        .ok_or_else(|| {
            ServerError::BadRequest("merchant seed not initialized, please re-authenticate".into())
        })?;

    let networks = kassi_db::queries::get_active_networks(&mut conn).await?;
    if networks.is_empty() {
        return Err(ServerError::BadRequest(
            "no active networks configured".into(),
        ));
    }

    let dep_id = EntityId::new(EntityPrefix::DepositAddress).to_string();
    let deposit_address = kassi_db::queries::insert_deposit_address(
        &mut conn,
        NewDepositAddress {
            id: &dep_id,
            merchant_id: &auth.merchant_id,
            label: body.label.as_deref(),
            address_type,
        },
    )
    .await?;

    let mut network_address_responses = Vec::with_capacity(networks.len());
    for network in &networks {
        let namespace_str = network
            .id
            .split(':')
            .next()
            .ok_or_else(|| ServerError::BadRequest("invalid network id format".into()))?;

        let next_index =
            kassi_db::queries::max_derivation_index(&mut conn, &auth.merchant_id, &network.id)
                .await?
                .map_or(0, |i| i + 1);

        let address = kassi_signer::derive_address(
            kms,
            &auth.merchant_id,
            &encrypted_seed,
            namespace_from_str(namespace_str)?,
            parse_chain_id_for_derivation(&network.id, namespace_str)?,
            next_index.cast_unsigned(),
        )
        .await
        .map_err(|e| ServerError::BadRequest(format!("address derivation failed: {e}")))?;

        let nadr_id = EntityId::new(EntityPrefix::NetworkAddress).to_string();
        kassi_db::queries::insert_network_address(
            &mut conn,
            NewNetworkAddress {
                id: &nadr_id,
                deposit_address_id: &dep_id,
                network_id: &network.id,
                address: &address,
                derivation_index: next_index,
            },
        )
        .await?;

        network_address_responses.push(NetworkAddressResponse {
            id: nadr_id,
            network: NetworkEmbed::from(network),
            address,
        });
    }

    Ok(ApiSuccess::created(deposit_address_response(
        deposit_address,
        network_address_responses,
    )))
}

async fn list_deposit_addresses(
    State(state): State<AppState>,
    auth: AnyAuth,
    Query(params): Query<PaginationParams>,
) -> Result<ApiList<DepositAddressResponse>, ServerError> {
    let limit = params.limit.unwrap_or(20).min(100);

    let mut conn = state
        .db
        .get()
        .await
        .map_err(|e| kassi_db::DbError::Pool(e.to_string()))?;

    let cursor = params.page.as_ref().map(|p| decode_cursor(p)).transpose()?;

    let mut query = schema::deposit_addresses::table
        .filter(schema::deposit_addresses::merchant_id.eq(&auth.merchant_id))
        .order((
            schema::deposit_addresses::created_at.desc(),
            schema::deposit_addresses::id.desc(),
        ))
        .select(DepositAddress::as_select())
        .limit(i64::try_from(limit + 1).unwrap_or(i64::MAX))
        .into_boxed();

    if let Some((cursor_time, cursor_id)) = &cursor {
        query = query.filter(
            schema::deposit_addresses::created_at.lt(cursor_time).or(
                schema::deposit_addresses::created_at
                    .eq(cursor_time)
                    .and(schema::deposit_addresses::id.lt(cursor_id)),
            ),
        );
    }

    let mut rows = query
        .load::<DepositAddress>(&mut conn)
        .await
        .map_err(kassi_db::DbError::from)?;

    let has_next = rows.len() > limit;
    if has_next {
        rows.truncate(limit);
    }

    let next_page = if has_next {
        rows.last().map(|da| encode_cursor(&da.created_at, &da.id))
    } else {
        None
    };

    let dep_ids: Vec<&str> = rows.iter().map(|d| d.id.as_str()).collect();
    let mut na_map = group_network_addresses(
        kassi_db::queries::load_network_addresses(&mut conn, &dep_ids).await?,
    );

    let data = rows
        .into_iter()
        .map(|da| {
            let nas = na_map.remove(&da.id).unwrap_or_default();
            deposit_address_response(da, nas)
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

async fn get_deposit_address(
    State(state): State<AppState>,
    auth: AnyAuth,
    Path(id): Path<String>,
) -> Result<ApiSuccess<DepositAddressResponse>, ServerError> {
    let mut conn = state
        .db
        .get()
        .await
        .map_err(|e| kassi_db::DbError::Pool(e.to_string()))?;

    let da = kassi_db::queries::get_deposit_address(&mut conn, &id, &auth.merchant_id)
        .await?
        .ok_or_else(|| ServerError::NotFound {
            entity: "deposit_address",
            id: id.clone(),
        })?;

    let mut na_map = group_network_addresses(
        kassi_db::queries::load_network_addresses(&mut conn, &[&da.id]).await?,
    );

    Ok(ApiSuccess {
        data: deposit_address_response(da.clone(), na_map.remove(&da.id).unwrap_or_default()),
    })
}

async fn list_ledger_entries(
    State(state): State<AppState>,
    auth: AnyAuth,
    Path(id): Path<String>,
    Query(params): Query<PaginationParams>,
) -> Result<ApiList<LedgerEntryResponse>, ServerError> {
    let limit = params.limit.unwrap_or(20).min(100);

    let mut conn = state
        .db
        .get()
        .await
        .map_err(|e| kassi_db::DbError::Pool(e.to_string()))?;

    let da = kassi_db::queries::get_deposit_address(&mut conn, &id, &auth.merchant_id)
        .await?
        .ok_or_else(|| ServerError::NotFound {
            entity: "deposit_address",
            id: id.clone(),
        })?;

    let dep_address_str = kassi_db::queries::first_network_address_string(&mut conn, &da.id)
        .await?
        .unwrap_or_default();

    let cursor = params.page.as_ref().map(|p| decode_cursor(p)).transpose()?;

    let mut query = schema::ledger_entries::table
        .filter(schema::ledger_entries::deposit_address_id.eq(&da.id))
        .order((
            schema::ledger_entries::created_at.desc(),
            schema::ledger_entries::id.desc(),
        ))
        .select(LedgerEntry::as_select())
        .limit(i64::try_from(limit + 1).unwrap_or(i64::MAX))
        .into_boxed();

    if let Some((cursor_time, cursor_id)) = &cursor {
        query = query.filter(
            schema::ledger_entries::created_at.lt(cursor_time).or(
                schema::ledger_entries::created_at
                    .eq(cursor_time)
                    .and(schema::ledger_entries::id.lt(cursor_id)),
            ),
        );
    }

    let mut entries = query
        .load::<LedgerEntry>(&mut conn)
        .await
        .map_err(kassi_db::DbError::from)?;

    let has_next = entries.len() > limit;
    if has_next {
        entries.truncate(limit);
    }

    let next_page = if has_next {
        entries.last().map(|e| encode_cursor(&e.created_at, &e.id))
    } else {
        None
    };

    let asset_ids: Vec<&str> = entries.iter().map(|e| e.asset_id.as_str()).collect();
    let network_ids: Vec<&str> = entries.iter().map(|e| e.network_id.as_str()).collect();
    let assets = kassi_db::queries::load_assets_by_ids(&mut conn, &asset_ids).await?;
    let networks = kassi_db::queries::load_networks_by_ids(&mut conn, &network_ids).await?;

    let asset_map: HashMap<&str, &kassi_db::models::Asset> =
        assets.iter().map(|a| (a.id.as_str(), a)).collect();
    let network_map: HashMap<&str, &Network> =
        networks.iter().map(|n| (n.id.as_str(), n)).collect();

    let data = entries
        .iter()
        .filter_map(|entry| {
            let asset = asset_map.get(entry.asset_id.as_str())?;
            let network = network_map.get(entry.network_id.as_str())?;

            Some(LedgerEntryResponse {
                id: entry.id.clone(),
                deposit_address: DepositAddressEmbed {
                    id: da.id.clone(),
                    address: dep_address_str.clone(),
                },
                payment_intent_id: entry.payment_intent_id.clone(),
                asset: AssetEmbed {
                    id: asset.id.clone(),
                    symbol: asset.symbol.clone(),
                    decimals: asset.decimals,
                },
                network: NetworkEmbed::from(*network),
                entry_type: entry.entry_type.clone(),
                status: entry.status.clone(),
                amount: entry.amount.clone(),
                fee_amount: entry.fee_amount.clone(),
                sender: entry.sender.clone(),
                destination: entry.destination.clone(),
                onchain_ref: entry.onchain_ref.clone(),
                reason: entry.reason.clone(),
                created_at: entry.created_at,
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
