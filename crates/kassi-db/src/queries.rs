use diesel::prelude::*;
use diesel_async::AsyncPgConnection;
use diesel_async::RunQueryDsl;

use crate::models::{
    Asset, DepositAddress, MerchantConfig, Network, NetworkAddress, NewDepositAddress, NewMerchant,
    NewMerchantConfig, NewNetworkAddress, NewSigner,
};
use crate::schema;
use crate::DbError;

/// Fetch a merchant's config by merchant ID.
///
/// # Errors
/// Returns `DbError::Query` if the query fails (including not found).
pub async fn get_merchant_config(
    conn: &mut AsyncPgConnection,
    merchant_id: &str,
) -> Result<MerchantConfig, DbError> {
    schema::merchant_configs::table
        .filter(schema::merchant_configs::merchant_id.eq(merchant_id))
        .select(MerchantConfig::as_select())
        .first::<MerchantConfig>(conn)
        .await
        .map_err(DbError::from)
}

/// Fetch all active networks.
///
/// # Errors
/// Returns `DbError::Query` if the query fails.
pub async fn get_active_networks(conn: &mut AsyncPgConnection) -> Result<Vec<Network>, DbError> {
    schema::networks::table
        .filter(schema::networks::is_active.eq(true))
        .select(Network::as_select())
        .load::<Network>(conn)
        .await
        .map_err(DbError::from)
}

/// Insert a deposit address and return the created row.
///
/// # Errors
/// Returns `DbError::Query` if the insert fails.
pub async fn insert_deposit_address(
    conn: &mut AsyncPgConnection,
    values: NewDepositAddress<'_>,
) -> Result<DepositAddress, DbError> {
    diesel::insert_into(schema::deposit_addresses::table)
        .values(values)
        .returning(DepositAddress::as_returning())
        .get_result::<DepositAddress>(conn)
        .await
        .map_err(DbError::from)
}

/// Insert a network address.
///
/// # Errors
/// Returns `DbError::Query` if the insert fails.
pub async fn insert_network_address(
    conn: &mut AsyncPgConnection,
    values: NewNetworkAddress<'_>,
) -> Result<(), DbError> {
    diesel::insert_into(schema::network_addresses::table)
        .values(values)
        .execute(conn)
        .await
        .map_err(DbError::from)?;
    Ok(())
}

/// Get the highest derivation index for a merchant + network combination.
/// Returns `None` if no addresses exist yet.
///
/// # Errors
/// Returns `DbError::Query` if the query fails.
pub async fn max_derivation_index(
    conn: &mut AsyncPgConnection,
    merchant_id: &str,
    network_id: &str,
) -> Result<Option<i32>, DbError> {
    schema::network_addresses::table
        .inner_join(schema::deposit_addresses::table.on(
            schema::deposit_addresses::id.eq(schema::network_addresses::deposit_address_id),
        ))
        .filter(schema::deposit_addresses::merchant_id.eq(merchant_id))
        .filter(schema::network_addresses::network_id.eq(network_id))
        .select(diesel::dsl::max(
            schema::network_addresses::derivation_index,
        ))
        .first(conn)
        .await
        .map_err(DbError::from)
}

/// Load network addresses joined with their networks for a set of deposit address IDs.
///
/// # Errors
/// Returns `DbError::Query` if the query fails.
pub async fn load_network_addresses(
    conn: &mut AsyncPgConnection,
    deposit_address_ids: &[&str],
) -> Result<Vec<(NetworkAddress, Network)>, DbError> {
    schema::network_addresses::table
        .inner_join(
            schema::networks::table
                .on(schema::networks::id.eq(schema::network_addresses::network_id)),
        )
        .filter(schema::network_addresses::deposit_address_id.eq_any(deposit_address_ids))
        .select((NetworkAddress::as_select(), Network::as_select()))
        .load::<(NetworkAddress, Network)>(conn)
        .await
        .map_err(DbError::from)
}

/// Fetch a single deposit address by ID, scoped to a merchant.
/// Returns `None` if not found.
///
/// # Errors
/// Returns `DbError::Query` if the query fails.
pub async fn get_deposit_address(
    conn: &mut AsyncPgConnection,
    id: &str,
    merchant_id: &str,
) -> Result<Option<DepositAddress>, DbError> {
    schema::deposit_addresses::table
        .filter(schema::deposit_addresses::id.eq(id))
        .filter(schema::deposit_addresses::merchant_id.eq(merchant_id))
        .select(DepositAddress::as_select())
        .first::<DepositAddress>(conn)
        .await
        .optional()
        .map_err(DbError::from)
}

/// Fetch the first network address string for a deposit address (for embeds).
///
/// # Errors
/// Returns `DbError::Query` if the query fails.
pub async fn first_network_address_string(
    conn: &mut AsyncPgConnection,
    deposit_address_id: &str,
) -> Result<Option<String>, DbError> {
    schema::network_addresses::table
        .filter(schema::network_addresses::deposit_address_id.eq(deposit_address_id))
        .select(schema::network_addresses::address)
        .first::<String>(conn)
        .await
        .optional()
        .map_err(DbError::from)
}

/// Batch-load assets by IDs.
///
/// # Errors
/// Returns `DbError::Query` if the query fails.
pub async fn load_assets_by_ids(
    conn: &mut AsyncPgConnection,
    ids: &[&str],
) -> Result<Vec<Asset>, DbError> {
    schema::assets::table
        .filter(schema::assets::id.eq_any(ids))
        .select(Asset::as_select())
        .load::<Asset>(conn)
        .await
        .map_err(DbError::from)
}

/// Batch-load networks by IDs.
///
/// # Errors
/// Returns `DbError::Query` if the query fails.
pub async fn load_networks_by_ids(
    conn: &mut AsyncPgConnection,
    ids: &[&str],
) -> Result<Vec<Network>, DbError> {
    schema::networks::table
        .filter(schema::networks::id.eq_any(ids))
        .select(Network::as_select())
        .load::<Network>(conn)
        .await
        .map_err(DbError::from)
}

/// Parameters for creating a new merchant with config and signer in a single transaction.
pub struct CreateMerchantParams<'a> {
    pub merchant_id: &'a str,
    pub config_id: &'a str,
    pub signer_id: &'a str,
    pub encrypted_seed: Option<&'a str>,
    pub webhook_secret: &'a str,
    pub signer_address: &'a str,
    pub signer_type: &'a str,
}

/// Create a merchant, config, and signer in a single transaction.
///
/// # Errors
/// Returns `DbError::Query` if any insert fails.
pub async fn create_merchant_with_config(
    conn: &mut AsyncPgConnection,
    params: CreateMerchantParams<'_>,
) -> Result<(), DbError> {
    conn.build_transaction()
        .run(|conn| {
            let params_merchant_id = params.merchant_id.to_string();
            let params_config_id = params.config_id.to_string();
            let params_signer_id = params.signer_id.to_string();
            let params_encrypted_seed = params.encrypted_seed.map(String::from);
            let params_webhook_secret = params.webhook_secret.to_string();
            let params_signer_address = params.signer_address.to_string();
            let params_signer_type = params.signer_type.to_string();

            Box::pin(async move {
                diesel::insert_into(schema::merchants::table)
                    .values(NewMerchant {
                        id: &params_merchant_id,
                    })
                    .execute(conn)
                    .await?;

                diesel::insert_into(schema::merchant_configs::table)
                    .values(NewMerchantConfig {
                        id: &params_config_id,
                        merchant_id: &params_merchant_id,
                        encrypted_seed: params_encrypted_seed.as_deref(),
                        webhook_secret: &params_webhook_secret,
                    })
                    .execute(conn)
                    .await?;

                diesel::insert_into(schema::signers::table)
                    .values(NewSigner {
                        id: &params_signer_id,
                        merchant_id: &params_merchant_id,
                        address: &params_signer_address,
                        signer_type: &params_signer_type,
                    })
                    .execute(conn)
                    .await?;

                Ok::<_, diesel::result::Error>(())
            })
        })
        .await
        .map_err(DbError::from)
}

/// Check whether a merchant exists by ID.
///
/// # Errors
/// Returns `DbError::Query` if the query fails.
pub async fn merchant_exists(
    conn: &mut AsyncPgConnection,
    merchant_id: &str,
) -> Result<bool, DbError> {
    let count = schema::merchants::table
        .filter(schema::merchants::id.eq(merchant_id))
        .count()
        .get_result::<i64>(conn)
        .await
        .map_err(DbError::from)?;
    Ok(count > 0)
}

/// Find a signer by address, returning `None` if not found.
///
/// # Errors
/// Returns `DbError::Query` if the query fails.
pub async fn find_signer_by_address(
    conn: &mut AsyncPgConnection,
    address: &str,
) -> Result<Option<crate::models::Signer>, DbError> {
    schema::signers::table
        .filter(schema::signers::address.eq(address))
        .select(crate::models::Signer::as_select())
        .first::<crate::models::Signer>(conn)
        .await
        .optional()
        .map_err(DbError::from)
}

/// Consume (delete) a valid, unexpired nonce. Returns `true` if consumed, `false` if not found.
///
/// # Errors
/// Returns `DbError::Query` if the query fails.
pub async fn consume_nonce(conn: &mut AsyncPgConnection, nonce: &str) -> Result<bool, DbError> {
    let deleted = diesel::delete(
        schema::nonces::table
            .filter(schema::nonces::nonce.eq(nonce))
            .filter(schema::nonces::expires_at.gt(chrono::Utc::now())),
    )
    .execute(conn)
    .await
    .map_err(DbError::from)?;
    Ok(deleted > 0)
}

/// Look up a merchant config by API key hash.
///
/// # Errors
/// Returns `DbError::Query` if the query fails.
pub async fn find_config_by_api_key_hash(
    conn: &mut AsyncPgConnection,
    hash: &str,
) -> Result<Option<MerchantConfig>, DbError> {
    schema::merchant_configs::table
        .filter(schema::merchant_configs::api_key_hash.eq(hash))
        .select(MerchantConfig::as_select())
        .first::<MerchantConfig>(conn)
        .await
        .optional()
        .map_err(DbError::from)
}
