package signer

import (
	"context"
	"crypto/ed25519"
	"fmt"
	"math/big"

	"github.com/btcsuite/btcd/btcutil/hdkeychain"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/gagliardetto/solana-go"
	"github.com/mr-tron/base58"
)

// KMS abstracts key management operations. InfisicalKMS is the production
// implementation, tests supply a mock.
type KMS interface {
	CreateKey(ctx context.Context, name string) error
	Encrypt(ctx context.Context, name string, plaintext []byte) (string, error)
	Decrypt(ctx context.Context, name string, ciphertext string) ([]byte, error)
}

func kmsKeyName(merchantID string) string {
	return "kassi-merchant-" + merchantID
}

// CreateMerchantSeed generates a BIP-32 master seed, encrypts it via KMS under
// a merchant-specific key, and returns the ciphertext for storage.
func CreateMerchantSeed(ctx context.Context, kms KMS, merchantID string) (string, error) {
	keyName := kmsKeyName(merchantID)

	if err := kms.CreateKey(ctx, keyName); err != nil {
		return "", fmt.Errorf("creating KMS key for merchant %s: %w", merchantID, err)
	}

	seed, err := hdkeychain.GenerateSeed(hdkeychain.RecommendedSeedLen)
	if err != nil {
		return "", fmt.Errorf("generating seed: %w", err)
	}

	ciphertext, err := kms.Encrypt(ctx, keyName, seed)
	clear(seed)
	if err != nil {
		return "", fmt.Errorf("encrypting seed for merchant %s: %w", merchantID, err)
	}

	return ciphertext, nil
}

func decryptSeed(ctx context.Context, kms KMS, merchantID, encryptedSeed string) ([]byte, error) {
	seed, err := kms.Decrypt(ctx, kmsKeyName(merchantID), encryptedSeed)
	if err != nil {
		return nil, fmt.Errorf("decrypting seed for merchant %s: %w", merchantID, err)
	}
	return seed, nil
}

// DeriveAddress decrypts the merchant seed and derives a public address for the
// given network and index. Returns checksummed hex for EVM, base58 for Solana.
func DeriveAddress(ctx context.Context, kms KMS, merchantID, encryptedSeed, networkID string, index uint32) (string, error) {
	net, err := LookupNetwork(networkID)
	if err != nil {
		return "", err
	}

	seed, err := decryptSeed(ctx, kms, merchantID, encryptedSeed)
	if err != nil {
		return "", err
	}
	defer clear(seed)

	switch net.ChainType {
	case "evm":
		return deriveEVMAddress(seed, net.CoinType, index)
	case "solana":
		key, err := deriveSolanaKey(seed, net.CoinType, index)
		if err != nil {
			return "", err
		}
		pub := ed25519.NewKeyFromSeed(key).Public().(ed25519.PublicKey)
		return base58.Encode(pub), nil
	default:
		return "", fmt.Errorf("unsupported chain type: %s", net.ChainType)
	}
}

// SignEVMTransaction decrypts the merchant seed, derives the private key at the
// given index, and returns the RLP-encoded signed transaction bytes.
func SignEVMTransaction(ctx context.Context, kms KMS, merchantID, encryptedSeed, networkID string, index uint32, tx *types.Transaction) ([]byte, error) {
	net, err := LookupNetwork(networkID)
	if err != nil {
		return nil, err
	}
	if net.ChainType != "evm" {
		return nil, fmt.Errorf("network %s is not EVM", networkID)
	}

	seed, err := decryptSeed(ctx, kms, merchantID, encryptedSeed)
	if err != nil {
		return nil, err
	}
	defer clear(seed)

	key, err := deriveEVMKey(seed, net.CoinType, index)
	if err != nil {
		return nil, err
	}

	privKey, err := key.ECPrivKey()
	if err != nil {
		return nil, fmt.Errorf("extracting EC private key: %w", err)
	}

	return signEVMTx(privKey.ToECDSA(), big.NewInt(int64(net.ChainID)), tx)
}

// SignSolanaTransaction decrypts the merchant seed, derives the ed25519 keypair
// at the given index, and partial-signs the transaction in place.
func SignSolanaTransaction(ctx context.Context, kms KMS, merchantID, encryptedSeed, networkID string, index uint32, tx *solana.Transaction) error {
	net, err := LookupNetwork(networkID)
	if err != nil {
		return err
	}
	if net.ChainType != "solana" {
		return fmt.Errorf("network %s is not Solana", networkID)
	}

	seed, err := decryptSeed(ctx, kms, merchantID, encryptedSeed)
	if err != nil {
		return err
	}
	defer clear(seed)

	key, err := deriveSolanaKey(seed, net.CoinType, index)
	if err != nil {
		return err
	}

	return signSolanaTransaction(solana.PrivateKey(ed25519.NewKeyFromSeed(key)), tx)
}
