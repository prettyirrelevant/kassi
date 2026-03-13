use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{DateTime, Utc};
use kassi_db::models::{Asset, DepositAddress, LedgerEntry, Network};
use kassi_signer::Namespace;
use serde::Serialize;

use crate::errors::ServerError;

// -- shared response embed types --

#[derive(Serialize)]
pub struct NetworkEmbed {
    pub id: String,
    pub display_name: String,
}

impl From<Network> for NetworkEmbed {
    fn from(n: Network) -> Self {
        Self {
            id: n.id,
            display_name: n.display_name,
        }
    }
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
pub struct AssetEmbed {
    pub id: String,
    pub symbol: String,
    pub decimals: i32,
}

impl From<Asset> for AssetEmbed {
    fn from(a: Asset) -> Self {
        Self {
            id: a.id,
            symbol: a.symbol,
            decimals: a.decimals,
        }
    }
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
pub struct DepositAddressEmbed {
    pub id: String,
    pub address: String,
}

impl DepositAddressEmbed {
    pub fn new(da: &DepositAddress, address: &str) -> Self {
        Self {
            id: da.id.clone(),
            address: address.to_string(),
        }
    }

    pub fn from_parts(id: String, address: String) -> Self {
        Self { id, address }
    }
}

#[derive(Serialize)]
pub struct LedgerEntryResponse {
    pub id: String,
    pub deposit_address: DepositAddressEmbed,
    pub payment_intent_id: Option<String>,
    pub asset: AssetEmbed,
    pub network: NetworkEmbed,
    pub entry_type: String,
    pub status: String,
    pub amount: String,
    pub fee_amount: Option<String>,
    pub sender: Option<String>,
    pub destination: Option<String>,
    pub onchain_ref: String,
    pub reason: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl LedgerEntryResponse {
    pub fn from_entry(
        entry: LedgerEntry,
        deposit_address: DepositAddressEmbed,
        asset: AssetEmbed,
        network: NetworkEmbed,
    ) -> Self {
        Self {
            id: entry.id,
            deposit_address,
            payment_intent_id: entry.payment_intent_id,
            asset,
            network,
            entry_type: entry.entry_type,
            status: entry.status,
            amount: entry.amount,
            fee_amount: entry.fee_amount,
            sender: entry.sender,
            destination: entry.destination,
            onchain_ref: entry.onchain_ref,
            reason: entry.reason,
            created_at: entry.created_at,
        }
    }
}

// -- shared helpers --

pub fn encode_cursor(created_at: &DateTime<Utc>, id: &str) -> String {
    URL_SAFE_NO_PAD.encode(format!("{}|{}", created_at.to_rfc3339(), id))
}

pub fn decode_cursor(cursor: &str) -> Result<(DateTime<Utc>, String), ServerError> {
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

pub fn parse_chain_id_for_derivation(
    network_id: &str,
    namespace: &str,
) -> Result<u64, ServerError> {
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

pub fn namespace_from_str(s: &str) -> Result<Namespace, ServerError> {
    match s {
        "eip155" => Ok(Namespace::Evm),
        "solana" => Ok(Namespace::Solana),
        _ => Err(ServerError::BadRequest(format!(
            "unsupported namespace: {s}"
        ))),
    }
}
