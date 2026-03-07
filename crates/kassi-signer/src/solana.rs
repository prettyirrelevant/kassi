use solana_signer::Signer;
use solana_transaction::Transaction;

use crate::derivation::derive_solana_keypair;
use crate::SignerError;

/// Sign a Solana transaction with the derived deposit keypair.
/// The transaction must already have its message constructed with the derived
/// pubkey in the account keys. This performs a partial sign, placing the
/// signature at the correct position.
pub(crate) fn sign_solana_tx(
    seed: &[u8],
    chain_id: u64,
    index: u32,
    tx: &mut Transaction,
) -> Result<(), SignerError> {
    let keypair = derive_solana_keypair(seed, chain_id, index)?;
    let pubkey = keypair.pubkey();

    let position = tx
        .message
        .account_keys
        .iter()
        .position(|k| k == &pubkey)
        .ok_or_else(|| {
            SignerError::Signing("derived pubkey not found in transaction account keys".into())
        })?;

    if position >= tx.signatures.len() {
        return Err(SignerError::Signing(
            "derived pubkey is not a required signer".into(),
        ));
    }

    let signature = keypair.sign_message(&tx.message_data());
    tx.signatures[position] = signature;

    Ok(())
}

#[cfg(test)]
mod tests {
    use solana_message::Message;
    use solana_pubkey::Pubkey;
    use solana_signature::Signature;
    use solana_signer::Signer;
    use solana_system_interface::instruction as system_instruction;
    use solana_transaction::Transaction;

    use super::*;
    use crate::derivation::derive_solana_keypair;

    const TEST_SEED: [u8; 64] = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
        0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e,
        0x1f, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x2d,
        0x2e, 0x2f, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x3b, 0x3c,
        0x3d, 0x3e, 0x3f, 0x40,
    ];

    #[test]
    fn sign_and_verify_signature() {
        let keypair = derive_solana_keypair(&TEST_SEED, 501, 0).unwrap();
        let deposit_pubkey = keypair.pubkey();
        let destination = Pubkey::new_unique();

        let ix = system_instruction::transfer(&deposit_pubkey, &destination, 1_000_000);
        let message = Message::new(&[ix], Some(&deposit_pubkey));
        let mut tx = Transaction::new_unsigned(message);
        tx.message.recent_blockhash = solana_hash::Hash::new_unique();
        tx.signatures =
            vec![Signature::default(); tx.message.header.num_required_signatures as usize];

        sign_solana_tx(&TEST_SEED, 501, 0, &mut tx).unwrap();

        assert!(tx.signatures[0] != Signature::default());
        assert!(tx.verify().is_ok());
    }

    #[test]
    fn fee_payer_is_separate_from_deposit_keypair() {
        let deposit = derive_solana_keypair(&TEST_SEED, 501, 0).unwrap();
        let relayer = derive_solana_keypair(&TEST_SEED, 501, 99).unwrap();
        let destination = Pubkey::new_unique();

        let ix = system_instruction::transfer(&deposit.pubkey(), &destination, 500_000);
        let message = Message::new(&[ix], Some(&relayer.pubkey()));
        let mut tx = Transaction::new_unsigned(message);
        tx.message.recent_blockhash = solana_hash::Hash::new_unique();
        tx.signatures =
            vec![Signature::default(); tx.message.header.num_required_signatures as usize];

        // sign with relayer first (index 99)
        sign_solana_tx(&TEST_SEED, 501, 99, &mut tx).unwrap();
        // sign with deposit keypair (index 0)
        sign_solana_tx(&TEST_SEED, 501, 0, &mut tx).unwrap();

        // fee payer (position 0 in account_keys) is the relayer, not deposit
        assert_eq!(tx.message.account_keys[0], relayer.pubkey());
        assert!(tx.verify().is_ok());
    }
}
