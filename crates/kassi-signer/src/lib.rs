mod derivation;
mod evm;
mod infisical;
mod solana;

use std::fmt;

use rand::RngCore;
use zeroize::Zeroize;

pub use derivation::{derive_evm_address, derive_solana_address};
pub use evm::{encode_erc20_transfer, encode_multicall3, MULTICALL3_ADDRESS};
pub use infisical::{InfisicalKms, KmsError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Namespace {
    Evm,
    Solana,
}

impl fmt::Display for Namespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Evm => f.write_str("evm"),
            Self::Solana => f.write_str("solana"),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SignerError {
    #[error("kms error: {0}")]
    Kms(#[from] KmsError),

    #[error("BIP-32 derivation error: {0}")]
    Bip32(#[from] bip32::Error),

    #[error("base64 decode error: {0}")]
    Base64(#[from] base64::DecodeError),

    #[error("signing error: {0}")]
    Signing(String),
}

/// Generate a new BIP-32 seed for a merchant, encrypt it via Infisical KMS,
/// and return the ciphertext. The caller stores the ciphertext in the database.
///
/// # Errors
/// Returns `SignerError::Kms` if key creation or encryption fails.
pub async fn create_merchant_seed(
    kms: &InfisicalKms,
    merchant_id: &str,
) -> Result<String, SignerError> {
    let key_name = InfisicalKms::key_name(merchant_id);
    kms.create_key(&key_name).await?;

    let mut seed = [0u8; 64];
    rand::thread_rng().fill_bytes(&mut seed);

    let ciphertext = kms.encrypt(&key_name, &seed).await?;
    seed.zeroize();

    tracing::info!(merchant_id, "created merchant seed");
    Ok(ciphertext)
}

/// Decrypt a merchant seed and derive a chain address.
/// Returns the address as a string (checksummed hex for EVM, base58 for Solana).
///
/// # Errors
/// Returns `SignerError` if decryption or key derivation fails.
pub async fn derive_address(
    kms: &InfisicalKms,
    merchant_id: &str,
    encrypted_seed: &str,
    namespace: Namespace,
    chain_id: u64,
    index: u32,
) -> Result<String, SignerError> {
    let key_name = InfisicalKms::key_name(merchant_id);
    let mut seed = kms.decrypt(&key_name, encrypted_seed).await?;

    let result = match namespace {
        Namespace::Evm => derive_evm_address(&seed, chain_id, index).map(|a| a.to_checksum(None)),
        Namespace::Solana => derive_solana_address(&seed, chain_id, index).map(|a| a.to_string()),
    };
    seed.zeroize();

    let address = result?;
    tracing::info!(merchant_id, %namespace, chain_id, index, "derived address");
    Ok(address)
}

/// Decrypt a merchant seed and sign an EVM transaction.
/// Returns EIP-2718 encoded signed transaction bytes.
///
/// # Errors
/// Returns `SignerError` if decryption, key derivation, or signing fails.
pub async fn sign_evm_transaction(
    kms: &InfisicalKms,
    merchant_id: &str,
    encrypted_seed: &str,
    chain_id: u64,
    index: u32,
    tx: alloy::rpc::types::TransactionRequest,
) -> Result<Vec<u8>, SignerError> {
    let key_name = InfisicalKms::key_name(merchant_id);
    let mut seed = kms.decrypt(&key_name, encrypted_seed).await?;

    let result = evm::sign_evm_tx(&seed, chain_id, index, tx).await;
    seed.zeroize();

    if result.is_ok() {
        tracing::info!(merchant_id, chain_id, index, "signed EVM transaction");
    }
    result
}

/// Decrypt a merchant seed and sign a Solana transaction.
/// The transaction is mutated in place with the deposit keypair's signature.
///
/// # Errors
/// Returns `SignerError` if decryption, key derivation, or signing fails.
pub async fn sign_solana_transaction(
    kms: &InfisicalKms,
    merchant_id: &str,
    encrypted_seed: &str,
    chain_id: u64,
    index: u32,
    tx: &mut solana_transaction::Transaction,
) -> Result<(), SignerError> {
    let key_name = InfisicalKms::key_name(merchant_id);
    let mut seed = kms.decrypt(&key_name, encrypted_seed).await?;

    let result = solana::sign_solana_tx(&seed, chain_id, index, tx);
    seed.zeroize();

    if result.is_ok() {
        tracing::info!(merchant_id, chain_id, index, "signed Solana transaction");
    }
    result
}
