package signer

import "github.com/gagliardetto/solana-go"

// signSolanaTransaction partial-signs a Solana transaction with the derived deposit keypair.
// The relayer signs separately as fee payer.
func signSolanaTransaction(privKey solana.PrivateKey, tx *solana.Transaction) error {
	_, err := tx.PartialSign(func(pub solana.PublicKey) *solana.PrivateKey {
		if pub.Equals(privKey.PublicKey()) {
			return &privKey
		}
		return nil
	})
	return err
}
