use chrono::{DateTime, Utc};
use diesel::prelude::*;

use crate::schema;

// -- networks --

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = schema::networks)]
pub struct Network {
    pub id: String,
    pub display_name: String,
    pub block_time_ms: i32,
    pub confirmations: i32,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

// -- merchants --

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = schema::merchants)]
pub struct Merchant {
    pub id: String,
    pub name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = schema::merchants)]
pub struct NewMerchant<'a> {
    pub id: &'a str,
}

// -- merchant_configs --

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = schema::merchant_configs)]
pub struct MerchantConfig {
    pub id: String,
    pub merchant_id: String,
    pub api_key_hash: Option<String>,
    pub encrypted_seed: Option<String>,
    pub webhook_secret: String,
    pub webhook_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = schema::merchant_configs)]
pub struct NewMerchantConfig<'a> {
    pub id: &'a str,
    pub merchant_id: &'a str,
    pub encrypted_seed: Option<&'a str>,
    pub webhook_secret: &'a str,
}

// -- settlement_destinations --

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = schema::settlement_destinations)]
pub struct SettlementDestination {
    pub id: String,
    pub merchant_id: String,
    pub network_id: String,
    pub address: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = schema::settlement_destinations)]
pub struct NewSettlementDestination<'a> {
    pub id: &'a str,
    pub merchant_id: &'a str,
    pub network_id: &'a str,
    pub address: &'a str,
}

// -- signers --

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = schema::signers)]
pub struct Signer {
    pub id: String,
    pub merchant_id: String,
    pub address: String,
    pub signer_type: String,
    pub linked_at: DateTime<Utc>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = schema::signers)]
pub struct NewSigner<'a> {
    pub id: &'a str,
    pub merchant_id: &'a str,
    pub address: &'a str,
    pub signer_type: &'a str,
}

// -- assets --

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = schema::assets)]
pub struct Asset {
    pub id: String,
    pub network_id: String,
    pub caip19: String,
    pub contract_address: Option<String>,
    pub symbol: String,
    pub name: String,
    pub decimals: i32,
    pub coingecko_id: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

// -- deposit_addresses --

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = schema::deposit_addresses)]
pub struct DepositAddress {
    pub id: String,
    pub merchant_id: String,
    pub label: Option<String>,
    pub address_type: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = schema::deposit_addresses)]
pub struct NewDepositAddress<'a> {
    pub id: &'a str,
    pub merchant_id: &'a str,
    pub label: Option<&'a str>,
    pub address_type: &'a str,
}

// -- network_addresses --

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = schema::network_addresses)]
pub struct NetworkAddress {
    pub id: String,
    pub deposit_address_id: String,
    pub network_id: String,
    pub address: String,
    pub derivation_index: i32,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = schema::network_addresses)]
pub struct NewNetworkAddress<'a> {
    pub id: &'a str,
    pub deposit_address_id: &'a str,
    pub network_id: &'a str,
    pub address: &'a str,
    pub derivation_index: i32,
}

// -- payment_intents --

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = schema::payment_intents)]
pub struct PaymentIntent {
    pub id: String,
    pub deposit_address_id: String,
    pub merchant_id: String,
    pub fiat_amount: String,
    pub fiat_currency: String,
    pub status: String,
    pub confirmed_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = schema::payment_intents)]
pub struct NewPaymentIntent<'a> {
    pub id: &'a str,
    pub deposit_address_id: &'a str,
    pub merchant_id: &'a str,
    pub fiat_amount: &'a str,
    pub fiat_currency: &'a str,
    pub expires_at: DateTime<Utc>,
}

// -- quotes --

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = schema::quotes)]
pub struct Quote {
    pub id: String,
    pub payment_intent_id: String,
    pub asset_id: String,
    pub exchange_rate: String,
    pub crypto_amount: String,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = schema::quotes)]
pub struct NewQuote<'a> {
    pub id: &'a str,
    pub payment_intent_id: &'a str,
    pub asset_id: &'a str,
    pub exchange_rate: &'a str,
    pub crypto_amount: &'a str,
    pub expires_at: DateTime<Utc>,
}

// -- ledger_entries --

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = schema::ledger_entries)]
pub struct LedgerEntry {
    pub id: String,
    pub deposit_address_id: String,
    pub payment_intent_id: Option<String>,
    pub asset_id: String,
    pub network_id: String,
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

#[derive(Debug, Insertable)]
#[diesel(table_name = schema::ledger_entries)]
pub struct NewLedgerEntry<'a> {
    pub id: &'a str,
    pub deposit_address_id: &'a str,
    pub payment_intent_id: Option<&'a str>,
    pub asset_id: &'a str,
    pub network_id: &'a str,
    pub entry_type: &'a str,
    pub status: &'a str,
    pub amount: &'a str,
    pub destination: Option<&'a str>,
    pub onchain_ref: &'a str,
    pub reason: Option<&'a str>,
}

// -- webhook_deliveries --

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = schema::webhook_deliveries)]
pub struct WebhookDelivery {
    pub id: String,
    pub merchant_id: String,
    pub event_type: String,
    pub reference_id: String,
    pub url: String,
    pub payload: serde_json::Value,
    pub status: String,
    pub attempts: i32,
    pub last_attempt_at: Option<DateTime<Utc>>,
    pub response_code: Option<i16>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// -- price_cache --

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = schema::price_cache)]
pub struct PriceCache {
    pub id: String,
    pub asset_id: String,
    pub fiat_currency: String,
    pub price: String,
    pub source: String,
    pub fetched_at: DateTime<Utc>,
}

// -- jobs --

#[derive(Debug, Clone, Queryable, Selectable, QueryableByName)]
#[diesel(table_name = schema::jobs)]
pub struct Job {
    pub id: i64,
    pub queue: String,
    pub payload: serde_json::Value,
    pub status: String,
    pub attempts: i32,
    pub max_attempts: i32,
    pub scheduled_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub failed_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = schema::jobs)]
pub struct NewJob<'a> {
    pub queue: &'a str,
    pub payload: serde_json::Value,
    pub max_attempts: i32,
    pub scheduled_at: Option<DateTime<Utc>>,
}

// -- nonces --

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = schema::nonces)]
pub struct Nonce {
    pub id: i64,
    pub nonce: String,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = schema::nonces)]
pub struct NewNonce<'a> {
    pub nonce: &'a str,
    pub expires_at: DateTime<Utc>,
}
