use alloy::primitives::Address;
use alloy::signers::local::PrivateKeySigner;
use bip32::{DerivationPath, XPrv};
use hmac::{Hmac, Mac};
use sha2::Sha512;
use solana_keypair::{keypair_from_seed, Keypair};
use solana_pubkey::Pubkey;
use solana_signer::Signer;

use crate::SignerError;

// -- EVM (BIP-32 / secp256k1) --

fn evm_derivation_path(chain_id: u64, index: u32) -> Result<DerivationPath, SignerError> {
    format!("m/44'/{chain_id}'/0'/0/{index}")
        .parse()
        .map_err(|_| SignerError::Signing("invalid EVM derivation path".into()))
}

/// Derive an EVM address from a seed, chain ID, and index using BIP-32/BIP-44.
///
/// # Errors
/// Returns `SignerError` if the derivation path is invalid or key derivation fails.
pub fn derive_evm_address(seed: &[u8], chain_id: u64, index: u32) -> Result<Address, SignerError> {
    Ok(derive_evm_signer(seed, chain_id, index)?.address())
}

pub(crate) fn derive_evm_signer(
    seed: &[u8],
    chain_id: u64,
    index: u32,
) -> Result<PrivateKeySigner, SignerError> {
    let path = evm_derivation_path(chain_id, index)?;
    let xprv = XPrv::derive_from_path(seed, &path)?;
    let key_bytes = xprv.private_key().to_bytes();
    PrivateKeySigner::from_slice(&key_bytes)
        .map_err(|e| SignerError::Signing(e.to_string()))
}

// -- Solana (SLIP-0010 / ed25519) --

type HmacSha512 = Hmac<Sha512>;

/// SLIP-0010 hardened derivation for ed25519.
/// All path segments are implicitly hardened (ed25519 only supports hardened derivation).
fn slip10_derive_ed25519(seed: &[u8], path: &[u32]) -> [u8; 32] {
    let mut mac = HmacSha512::new_from_slice(b"ed25519 seed")
        .expect("HMAC accepts any key length");
    mac.update(seed);
    let result = mac.finalize().into_bytes();

    let mut key = [0u8; 32];
    let mut chain_code = [0u8; 32];
    key.copy_from_slice(&result[..32]);
    chain_code.copy_from_slice(&result[32..]);

    for &segment in path {
        let mut mac = HmacSha512::new_from_slice(&chain_code)
            .expect("HMAC accepts any key length");
        mac.update(&[0x00]);
        mac.update(&key);
        mac.update(&(segment | 0x8000_0000).to_be_bytes());
        let result = mac.finalize().into_bytes();
        key.copy_from_slice(&result[..32]);
        chain_code.copy_from_slice(&result[32..]);
    }

    key
}

/// Derive a Solana address from a seed, chain ID, and index using SLIP-0010.
///
/// # Errors
/// Returns `SignerError` if key derivation fails.
pub fn derive_solana_address(
    seed: &[u8],
    chain_id: u64,
    index: u32,
) -> Result<Pubkey, SignerError> {
    Ok(derive_solana_keypair(seed, chain_id, index)?.pubkey())
}

pub(crate) fn derive_solana_keypair(
    seed: &[u8],
    chain_id: u64,
    index: u32,
) -> Result<Keypair, SignerError> {
    // m/44'/chain_id'/0'/0'/index' (all hardened for ed25519)
    let chain_id_u32 = u32::try_from(chain_id)
        .map_err(|_| SignerError::Signing("chain_id exceeds u32 range for SLIP-0010 path".into()))?;
    let path = [44, chain_id_u32, 0, 0, index];
    let key_bytes = slip10_derive_ed25519(seed, &path);
    keypair_from_seed(&key_bytes)
        .map_err(|e| SignerError::Signing(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SEED: [u8; 64] = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
        0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
        0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18,
        0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20,
        0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28,
        0x29, 0x2a, 0x2b, 0x2c, 0x2d, 0x2e, 0x2f, 0x30,
        0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38,
        0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f, 0x40,
    ];

    mod evm {
        use super::*;

        #[test]
        fn deterministic_derivation() {
            let addr1 = derive_evm_address(&TEST_SEED, 1, 0).unwrap();
            let addr2 = derive_evm_address(&TEST_SEED, 1, 0).unwrap();
            assert_eq!(addr1, addr2);
        }

        #[test]
        fn different_index_different_address() {
            let addr0 = derive_evm_address(&TEST_SEED, 1, 0).unwrap();
            let addr1 = derive_evm_address(&TEST_SEED, 1, 1).unwrap();
            assert_ne!(addr0, addr1);
        }

        #[test]
        fn different_chain_different_address() {
            let eth = derive_evm_address(&TEST_SEED, 1, 0).unwrap();
            let polygon = derive_evm_address(&TEST_SEED, 137, 0).unwrap();
            assert_ne!(eth, polygon);
        }

        #[test]
        fn address_is_valid_checksummed_hex() {
            let addr = derive_evm_address(&TEST_SEED, 1, 0).unwrap();
            let checksummed = addr.to_checksum(None);
            assert!(checksummed.starts_with("0x"));
            assert_eq!(checksummed.len(), 42);
        }
    }

    mod solana {
        use super::*;

        #[test]
        fn deterministic_derivation() {
            let addr1 = derive_solana_address(&TEST_SEED, 501, 0).unwrap();
            let addr2 = derive_solana_address(&TEST_SEED, 501, 0).unwrap();
            assert_eq!(addr1, addr2);
        }

        #[test]
        fn different_index_different_address() {
            let addr0 = derive_solana_address(&TEST_SEED, 501, 0).unwrap();
            let addr1 = derive_solana_address(&TEST_SEED, 501, 1).unwrap();
            assert_ne!(addr0, addr1);
        }

        #[test]
        fn address_is_valid_base58() {
            let addr = derive_solana_address(&TEST_SEED, 501, 0).unwrap();
            let reparsed: Pubkey = addr.to_string().parse().unwrap();
            assert_eq!(addr, reparsed);
        }
    }
}
