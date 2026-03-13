use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{DateTime, Duration, Utc};
use kassi_db::diesel::prelude::*;
use kassi_db::diesel_async::RunQueryDsl;
use kassi_db::models::{
    Asset, DepositAddress, LedgerEntry, Network, NetworkAddress, NewDepositAddress,
    NewNetworkAddress, NewPaymentIntent, NewQuote, PaymentIntent,
};
use kassi_db::schema;
use kassi_signer::Namespace;
use kassi_types::{EntityId, EntityPrefix};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

use crate::errors::{ServerError, ValidationDetail};
use crate::extractors::AnyAuth;
use crate::response::{ApiList, ApiSuccess, ListMeta};
use crate::AppState;

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
struct QuoteResponse {
    id: String,
    asset: AssetEmbed,
    exchange_rate: String,
    crypto_amount: String,
    expires_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
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

#[derive(Serialize)]
struct PaymentIntentResponse {
    id: String,
    deposit_address: DepositAddressEmbed,
    merchant_id: String,
    fiat_amount: String,
    fiat_currency: String,
    status: String,
    quotes: Vec<QuoteResponse>,
    confirmed_at: Option<DateTime<Utc>>,
    expires_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct PaymentIntentListItem {
    id: String,
    deposit_address: DepositAddressEmbed,
    merchant_id: String,
    fiat_amount: String,
    fiat_currency: String,
    status: String,
    quotes: Vec<QuoteResponse>,
    confirmed_at: Option<DateTime<Utc>>,
    expires_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct PaymentIntentDetailResponse {
    id: String,
    deposit_address: DepositAddressEmbed,
    merchant_id: String,
    fiat_amount: String,
    fiat_currency: String,
    status: String,
    quotes: Vec<QuoteResponse>,
    ledger_entries: Vec<LedgerEntryResponse>,
    confirmed_at: Option<DateTime<Utc>>,
    expires_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

// -- request types --

#[derive(Deserialize)]
struct CreateRequest {
    asset_id: Option<String>,
    fiat_amount: Option<String>,
    fiat_currency: Option<String>,
}

#[derive(Deserialize)]
struct ListParams {
    page: Option<String>,
    limit: Option<usize>,
    status: Option<String>,
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

const VALID_STATUSES: &[&str] = &["pending", "partial", "confirmed", "expired"];

const SUPPORTED_FIAT_CURRENCIES: &[&str] = &["USD"];

/// Compute the crypto amount from a fiat amount and exchange rate.
///
/// `fiat_amount`: human-readable fiat string (e.g. "25.00")
/// `exchange_rate`: USD price per one whole token (e.g. "1.0168")
/// `decimals`: token decimals (e.g. 6 for USDC)
///
/// Returns the crypto amount in the token's smallest unit as a string.
fn compute_crypto_amount(
    fiat_amount: &str,
    exchange_rate: &str,
    decimals: i32,
) -> Result<String, ServerError> {
    let fiat = Decimal::from_str(fiat_amount)
        .map_err(|_| ServerError::BadRequest("invalid fiat_amount".into()))?;
    let rate = Decimal::from_str(exchange_rate)
        .map_err(|_| ServerError::BadRequest("invalid exchange rate".into()))?;

    if rate.is_zero() {
        return Err(ServerError::BadRequest(
            "exchange rate cannot be zero".into(),
        ));
    }

    let one_token = Decimal::from(10_u128.pow(decimals.unsigned_abs()));
    // crypto_amount = floor(fiat_amount / exchange_rate * 10^decimals)
    let crypto = (fiat / rate * one_token).floor();

    Ok(crypto.to_string())
}

fn build_deposit_address_embed(
    da: &DepositAddress,
    address: &str,
) -> DepositAddressEmbed {
    DepositAddressEmbed {
        id: da.id.clone(),
        address: address.to_string(),
    }
}

// -- routes --

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/payment-intents",
            post(create_payment_intent).get(list_payment_intents),
        )
        .route("/payment-intents/{id}", get(get_payment_intent))
}

async fn create_payment_intent(
    State(state): State<AppState>,
    auth: AnyAuth,
    Json(body): Json<CreateRequest>,
) -> Result<axum::response::Response, ServerError> {
    // -- validate request --
    let mut errors = Vec::new();

    let asset_id = match &body.asset_id {
        Some(v) if !v.is_empty() => v.as_str(),
        _ => {
            errors.push(ValidationDetail {
                field: "asset_id".into(),
                code: "required",
                message: "this field is required.".into(),
            });
            ""
        }
    };

    let fiat_amount = match &body.fiat_amount {
        Some(v) if !v.is_empty() => v.as_str(),
        _ => {
            errors.push(ValidationDetail {
                field: "fiat_amount".into(),
                code: "required",
                message: "this field is required.".into(),
            });
            ""
        }
    };

    let fiat_currency = match &body.fiat_currency {
        Some(v) if !v.is_empty() => v.as_str(),
        _ => {
            errors.push(ValidationDetail {
                field: "fiat_currency".into(),
                code: "required",
                message: "this field is required.".into(),
            });
            ""
        }
    };

    // validate fiat_amount is a positive decimal
    if !fiat_amount.is_empty() {
        match Decimal::from_str(fiat_amount) {
            Ok(d) if d > Decimal::ZERO => {}
            _ => {
                errors.push(ValidationDetail {
                    field: "fiat_amount".into(),
                    code: "invalid_field_value",
                    message: "must be a positive decimal string.".into(),
                });
            }
        }
    }

    // validate fiat_currency
    if !fiat_currency.is_empty() && !SUPPORTED_FIAT_CURRENCIES.contains(&fiat_currency) {
        errors.push(ValidationDetail {
            field: "fiat_currency".into(),
            code: "invalid_field_value",
            message: "unsupported currency.".into(),
        });
    }

    if !errors.is_empty() {
        return Err(ServerError::ValidationFailed(errors));
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

    // validate asset exists and is active
    let asset = kassi_db::queries::get_asset_by_id(&mut conn, asset_id)
        .await?
        .ok_or_else(|| ServerError::ValidationFailed(vec![ValidationDetail {
            field: "asset_id".into(),
            code: "invalid_field_value",
            message: format!("no active asset found with id '{asset_id}'."),
        }]))?;

    // fetch exchange rate
    let coingecko_id = asset.coingecko_id.as_deref().ok_or_else(|| {
        ServerError::BadRequest(format!(
            "asset '{asset_id}' has no coingecko_id configured for pricing."
        ))
    })?;

    let prices = state
        .prices
        .fetch_prices(&[coingecko_id.to_string()])
        .await
        .map_err(|e| {
            ServerError::BadRequest(format!(
                "failed to fetch price for asset '{asset_id}': {e}"
            ))
        })?;

    let usd_price = prices
        .into_iter()
        .next()
        .ok_or_else(|| {
            ServerError::BadRequest(format!(
                "no price returned for asset '{asset_id}'."
            ))
        })?
        .usd_price;

    // format with full precision to avoid f64 display artifacts
    let exchange_rate = format!("{usd_price:.10}").trim_end_matches('0').trim_end_matches('.').to_string();

    // cache the fetched price
    let cache_id = EntityId::new(EntityPrefix::PriceCache).to_string();
    let _ = kassi_db::queries::upsert_price_cache(
        &mut conn,
        &cache_id,
        &asset.id,
        fiat_currency,
        &exchange_rate,
        "live",
        Utc::now(),
    )
    .await;

    let crypto_amount = compute_crypto_amount(fiat_amount, &exchange_rate, asset.decimals)?;

    // get merchant config for encrypted seed
    let encrypted_seed = kassi_db::queries::get_merchant_config(&mut conn, &auth.merchant_id)
        .await?
        .encrypted_seed
        .ok_or_else(|| {
            ServerError::BadRequest("merchant seed not initialized, please re-authenticate".into())
        })?;

    // get active networks (only the network for this asset)
    let network = schema::networks::table
        .filter(schema::networks::id.eq(&asset.network_id))
        .filter(schema::networks::is_active.eq(true))
        .select(Network::as_select())
        .first::<Network>(&mut conn)
        .await
        .optional()
        .map_err(kassi_db::DbError::from)?
        .ok_or_else(|| {
            ServerError::BadRequest(format!(
                "network '{}' is not active.",
                asset.network_id
            ))
        })?;

    // create one_off deposit address
    let dep_id = EntityId::new(EntityPrefix::DepositAddress).to_string();
    let deposit_address = kassi_db::queries::insert_deposit_address(
        &mut conn,
        NewDepositAddress {
            id: &dep_id,
            merchant_id: &auth.merchant_id,
            label: None,
            address_type: "one_off",
        },
    )
    .await?;

    // derive address for the asset's network
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

    // create payment intent
    let quote_duration_secs = state.config.quote_lock_duration_secs;
    let now = Utc::now();
    let expires_at = now + Duration::seconds(quote_duration_secs as i64);

    let pi_id = EntityId::new(EntityPrefix::PaymentIntent).to_string();
    let payment_intent = kassi_db::queries::insert_payment_intent(
        &mut conn,
        NewPaymentIntent {
            id: &pi_id,
            deposit_address_id: &dep_id,
            merchant_id: &auth.merchant_id,
            fiat_amount,
            fiat_currency,
            expires_at,
        },
    )
    .await?;

    // create quote
    let quo_id = EntityId::new(EntityPrefix::Quote).to_string();
    let quote = kassi_db::queries::insert_quote(
        &mut conn,
        NewQuote {
            id: &quo_id,
            payment_intent_id: &pi_id,
            asset_id: &asset.id,
            exchange_rate: &exchange_rate,
            crypto_amount: &crypto_amount,
            expires_at,
        },
    )
    .await?;

    let response = PaymentIntentResponse {
        id: payment_intent.id,
        deposit_address: build_deposit_address_embed(&deposit_address, &address),
        merchant_id: payment_intent.merchant_id,
        fiat_amount: payment_intent.fiat_amount,
        fiat_currency: payment_intent.fiat_currency,
        status: payment_intent.status,
        quotes: vec![QuoteResponse {
            id: quote.id,
            asset: AssetEmbed::from(&asset),
            exchange_rate: quote.exchange_rate,
            crypto_amount: quote.crypto_amount,
            expires_at: quote.expires_at,
            created_at: quote.created_at,
        }],
        confirmed_at: payment_intent.confirmed_at,
        expires_at: payment_intent.expires_at,
        created_at: payment_intent.created_at,
        updated_at: payment_intent.updated_at,
    };

    Ok(ApiSuccess::created(response))
}

async fn list_payment_intents(
    State(state): State<AppState>,
    auth: AnyAuth,
    Query(params): Query<ListParams>,
) -> Result<ApiList<PaymentIntentListItem>, ServerError> {
    let limit = params.limit.unwrap_or(20).min(100);

    // validate status filter
    if let Some(status) = &params.status {
        if !VALID_STATUSES.contains(&status.as_str()) {
            return Err(ServerError::ValidationFailed(vec![ValidationDetail {
                field: "status".into(),
                code: "invalid_field_value",
                message: format!(
                    "must be one of: {}.",
                    VALID_STATUSES.join(", ")
                ),
            }]));
        }
    }

    let mut conn = state
        .db
        .get()
        .await
        .map_err(|e| kassi_db::DbError::Pool(e.to_string()))?;

    let cursor = params.page.as_ref().map(|p| decode_cursor(p)).transpose()?;

    let mut query = schema::payment_intents::table
        .filter(schema::payment_intents::merchant_id.eq(&auth.merchant_id))
        .order((
            schema::payment_intents::created_at.desc(),
            schema::payment_intents::id.desc(),
        ))
        .select(PaymentIntent::as_select())
        .limit(i64::try_from(limit + 1).unwrap_or(i64::MAX))
        .into_boxed();

    if let Some(status) = &params.status {
        query = query.filter(schema::payment_intents::status.eq(status));
    }

    if let Some((cursor_time, cursor_id)) = &cursor {
        query = query.filter(
            schema::payment_intents::created_at
                .lt(cursor_time)
                .or(schema::payment_intents::created_at
                    .eq(cursor_time)
                    .and(schema::payment_intents::id.lt(cursor_id))),
        );
    }

    let mut rows = query
        .load::<PaymentIntent>(&mut conn)
        .await
        .map_err(kassi_db::DbError::from)?;

    let has_next = rows.len() > limit;
    if has_next {
        rows.truncate(limit);
    }

    let next_page = if has_next {
        rows.last()
            .map(|pi| encode_cursor(&pi.created_at, &pi.id))
    } else {
        None
    };

    // load first network address for each deposit address (for embed)
    let dep_ids: Vec<&str> = rows.iter().map(|pi| pi.deposit_address_id.as_str()).collect();
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

    // load quotes for all payment intents
    let pi_ids: Vec<&str> = rows.iter().map(|pi| pi.id.as_str()).collect();
    let quotes =
        kassi_db::queries::load_quotes_by_payment_intent_ids(&mut conn, &pi_ids).await?;

    // load assets for quotes
    let asset_ids: Vec<&str> = quotes.iter().map(|q| q.asset_id.as_str()).collect();
    let assets = kassi_db::queries::load_assets_by_ids(&mut conn, &asset_ids).await?;
    let asset_map: HashMap<&str, &Asset> = assets.iter().map(|a| (a.id.as_str(), a)).collect();

    // group quotes by payment_intent_id
    let mut quotes_map: HashMap<&str, Vec<QuoteResponse>> = HashMap::new();
    for q in &quotes {
        if let Some(asset) = asset_map.get(q.asset_id.as_str()) {
            quotes_map
                .entry(q.payment_intent_id.as_str())
                .or_default()
                .push(QuoteResponse {
                    id: q.id.clone(),
                    asset: AssetEmbed::from(*asset),
                    exchange_rate: q.exchange_rate.clone(),
                    crypto_amount: q.crypto_amount.clone(),
                    expires_at: q.expires_at,
                    created_at: q.created_at,
                });
        }
    }

    let data = rows
        .into_iter()
        .map(|pi| {
            let address_str = na_map
                .get(pi.deposit_address_id.as_str())
                .copied()
                .unwrap_or("");

            PaymentIntentListItem {
                id: pi.id.clone(),
                deposit_address: DepositAddressEmbed {
                    id: pi.deposit_address_id.clone(),
                    address: address_str.to_string(),
                },
                merchant_id: pi.merchant_id,
                fiat_amount: pi.fiat_amount,
                fiat_currency: pi.fiat_currency,
                status: pi.status,
                quotes: quotes_map.remove(pi.id.as_str()).unwrap_or_default(),
                confirmed_at: pi.confirmed_at,
                expires_at: pi.expires_at,
                created_at: pi.created_at,
                updated_at: pi.updated_at,
            }
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

async fn get_payment_intent(
    State(state): State<AppState>,
    auth: AnyAuth,
    Path(id): Path<String>,
) -> Result<ApiSuccess<PaymentIntentDetailResponse>, ServerError> {
    let mut conn = state
        .db
        .get()
        .await
        .map_err(|e| kassi_db::DbError::Pool(e.to_string()))?;

    let pi = kassi_db::queries::get_payment_intent(&mut conn, &id, &auth.merchant_id)
        .await?
        .ok_or_else(|| ServerError::NotFound {
            entity: "payment_intent",
            id: id.clone(),
        })?;

    // load deposit address embed
    let da = schema::deposit_addresses::table
        .filter(schema::deposit_addresses::id.eq(&pi.deposit_address_id))
        .select(DepositAddress::as_select())
        .first::<DepositAddress>(&mut conn)
        .await
        .map_err(kassi_db::DbError::from)?;

    let dep_address_str =
        kassi_db::queries::first_network_address_string(&mut conn, &da.id)
            .await?
            .unwrap_or_default();

    // load quotes
    let quotes =
        kassi_db::queries::load_quotes_by_payment_intent_ids(&mut conn, &[pi.id.as_str()])
            .await?;

    let asset_ids: Vec<&str> = quotes.iter().map(|q| q.asset_id.as_str()).collect();
    let assets = kassi_db::queries::load_assets_by_ids(&mut conn, &asset_ids).await?;
    let asset_map: HashMap<&str, &Asset> = assets.iter().map(|a| (a.id.as_str(), a)).collect();

    let quote_responses: Vec<QuoteResponse> = quotes
        .iter()
        .filter_map(|q| {
            let asset = asset_map.get(q.asset_id.as_str())?;
            Some(QuoteResponse {
                id: q.id.clone(),
                asset: AssetEmbed::from(*asset),
                exchange_rate: q.exchange_rate.clone(),
                crypto_amount: q.crypto_amount.clone(),
                expires_at: q.expires_at,
                created_at: q.created_at,
            })
        })
        .collect();

    // load ledger entries for this payment intent
    let ledger_entries = schema::ledger_entries::table
        .filter(schema::ledger_entries::payment_intent_id.eq(&pi.id))
        .order((
            schema::ledger_entries::created_at.desc(),
            schema::ledger_entries::id.desc(),
        ))
        .select(LedgerEntry::as_select())
        .load::<LedgerEntry>(&mut conn)
        .await
        .map_err(kassi_db::DbError::from)?;

    // load assets and networks for ledger entries
    let le_asset_ids: Vec<&str> = ledger_entries
        .iter()
        .map(|e| e.asset_id.as_str())
        .collect();
    let le_network_ids: Vec<&str> = ledger_entries
        .iter()
        .map(|e| e.network_id.as_str())
        .collect();

    let le_assets = kassi_db::queries::load_assets_by_ids(&mut conn, &le_asset_ids).await?;
    let le_networks = kassi_db::queries::load_networks_by_ids(&mut conn, &le_network_ids).await?;

    let le_asset_map: HashMap<&str, &Asset> =
        le_assets.iter().map(|a| (a.id.as_str(), a)).collect();
    let le_network_map: HashMap<&str, &Network> =
        le_networks.iter().map(|n| (n.id.as_str(), n)).collect();

    let ledger_entry_responses: Vec<LedgerEntryResponse> = ledger_entries
        .iter()
        .filter_map(|entry| {
            let asset = le_asset_map.get(entry.asset_id.as_str())?;
            let network = le_network_map.get(entry.network_id.as_str())?;
            Some(LedgerEntryResponse {
                id: entry.id.clone(),
                deposit_address: DepositAddressEmbed {
                    id: da.id.clone(),
                    address: dep_address_str.clone(),
                },
                payment_intent_id: entry.payment_intent_id.clone(),
                asset: AssetEmbed::from(*asset),
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

    Ok(ApiSuccess {
        data: PaymentIntentDetailResponse {
            id: pi.id,
            deposit_address: build_deposit_address_embed(&da, &dep_address_str),
            merchant_id: pi.merchant_id,
            fiat_amount: pi.fiat_amount,
            fiat_currency: pi.fiat_currency,
            status: pi.status,
            quotes: quote_responses,
            ledger_entries: ledger_entry_responses,
            confirmed_at: pi.confirmed_at,
            expires_at: pi.expires_at,
            created_at: pi.created_at,
            updated_at: pi.updated_at,
        },
    })
}
