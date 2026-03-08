use alloy::network::{EthereumWallet, TransactionBuilder};
use alloy::primitives::{Address, Bytes, U256};
use alloy::rpc::types::TransactionRequest;
use alloy::sol;
use alloy::sol_types::SolCall;

use crate::derivation::derive_evm_signer;
use crate::SignerError;

sol! {
    function transfer(address to, uint256 amount) external returns (bool);

    struct Call3 {
        address target;
        bool allowFailure;
        bytes callData;
    }

    function aggregate3(Call3[] calldata calls) external payable returns (bytes[] memory returnData);
}

/// Multicall3 canonical address (deployed on all EVM chains).
pub const MULTICALL3_ADDRESS: Address =
    alloy::primitives::address!("cA11bde05977b3631167028862bE2a173976CA11");

/// Encode an ERC-20 `transfer(address,uint256)` call.
#[must_use]
pub fn encode_erc20_transfer(to: Address, amount: U256) -> Bytes {
    Bytes::from(transferCall { to, amount }.abi_encode())
}

/// Build a Multicall3 `aggregate3` calldata from a list of calls.
#[must_use]
pub fn encode_multicall3(calls: &[Call3]) -> Bytes {
    Bytes::from(
        aggregate3Call {
            calls: calls.to_vec(),
        }
        .abi_encode(),
    )
}

/// Sign a fully-populated EVM transaction and return EIP-2718 encoded bytes.
pub(crate) async fn sign_evm_tx(
    seed: &[u8],
    chain_id: u64,
    index: u32,
    tx: TransactionRequest,
) -> Result<Vec<u8>, SignerError> {
    use alloy::network::eip2718::Encodable2718;

    let signer = derive_evm_signer(seed, chain_id, index)?;
    let wallet = EthereumWallet::from(signer);
    let tx_envelope = tx
        .build(&wallet)
        .await
        .map_err(|e| SignerError::Signing(e.to_string()))?;
    Ok(tx_envelope.encoded_2718())
}

#[cfg(test)]
mod tests {
    use alloy::consensus::transaction::SignerRecoverable;
    use alloy::network::eip2718::Decodable2718;
    use alloy::primitives::address;

    use super::*;
    use crate::derivation::derive_evm_signer;

    const TEST_SEED: [u8; 64] = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
        0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e,
        0x1f, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x2d,
        0x2e, 0x2f, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x3b, 0x3c,
        0x3d, 0x3e, 0x3f, 0x40,
    ];

    #[tokio::test]
    async fn sign_and_recover_signer() {
        let signer = derive_evm_signer(&TEST_SEED, 1, 0).unwrap();
        let expected_address = signer.address();

        let tx = TransactionRequest::default()
            .with_to(address!("d8dA6BF26964aF9D7eEd9e03E53415D37aA96045"))
            .with_nonce(0)
            .with_chain_id(1)
            .with_value(U256::from(100))
            .with_gas_limit(21_000)
            .with_max_priority_fee_per_gas(1_000_000_000)
            .with_max_fee_per_gas(20_000_000_000);

        let signed_bytes = sign_evm_tx(&TEST_SEED, 1, 0, tx).await.unwrap();
        assert!(!signed_bytes.is_empty());

        // Decode and recover signer to verify it matches the derived address
        let decoded: alloy::consensus::TxEnvelope =
            Decodable2718::decode_2718(&mut signed_bytes.as_slice()).unwrap();
        let recovered = decoded.recover_signer().unwrap();
        assert_eq!(recovered, expected_address);
    }

    #[test]
    fn erc20_transfer_encoding() {
        let to = address!("d8dA6BF26964aF9D7eEd9e03E53415D37aA96045");
        let amount = U256::from(1_000_000);
        let calldata = encode_erc20_transfer(to, amount);

        // ERC-20 transfer selector: 0xa9059cbb
        assert_eq!(&calldata[..4], &[0xa9, 0x05, 0x9c, 0xbb]);
    }

    #[test]
    fn multicall3_encoding_two_transfers() {
        let token = address!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");
        let recipient_a = address!("d8dA6BF26964aF9D7eEd9e03E53415D37aA96045");
        let recipient_b = address!("1234567890abcdef1234567890abcdef12345678");

        let calls = vec![
            Call3 {
                target: token,
                allowFailure: false,
                callData: encode_erc20_transfer(recipient_a, U256::from(900_000)),
            },
            Call3 {
                target: token,
                allowFailure: false,
                callData: encode_erc20_transfer(recipient_b, U256::from(100_000)),
            },
        ];

        let calldata = encode_multicall3(&calls);
        // aggregate3 selector: 0x82ad56cb
        assert_eq!(&calldata[..4], &[0x82, 0xad, 0x56, 0xcb]);
    }
}
