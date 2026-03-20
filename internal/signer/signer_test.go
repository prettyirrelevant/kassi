package signer

import (
	"context"
	"crypto/ed25519"
	"encoding/hex"
	"testing"

	"github.com/btcsuite/btcd/btcutil/hdkeychain"
	"github.com/mr-tron/base58"
	"github.com/stretchr/testify/mock"
	"github.com/stretchr/testify/require"

	"github.com/prettyirrelevant/kassi/internal/mocks"
)

var testSeed, _ = hex.DecodeString("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f")

func TestNetworkRegistry(t *testing.T) {
	t.Run("all entries have valid chain type", func(t *testing.T) {
		for id, info := range networks {
			require.Contains(t, []string{"evm", "solana"}, info.ChainType, "network %s has invalid chain type", id)
		}
	})

	t.Run("evm networks must have non-zero chain ID", func(t *testing.T) {
		for id, info := range networks {
			if info.ChainType == "evm" {
				require.NotZero(t, info.ChainID, "evm network %s has zero chain ID", id)
			}
		}
	})

	t.Run("evm networks must have coin type 60", func(t *testing.T) {
		for id, info := range networks {
			if info.ChainType == "evm" {
				require.Equal(t, uint32(60), info.CoinType, "evm network %s", id)
			}
		}
	})

	t.Run("solana networks must have coin type 501", func(t *testing.T) {
		for id, info := range networks {
			if info.ChainType == "solana" {
				require.Equal(t, uint32(501), info.CoinType, "solana network %s", id)
			}
		}
	})

	t.Run("no duplicate chain IDs among evm networks", func(t *testing.T) {
		seen := make(map[uint64]string)
		for id, info := range networks {
			if info.ChainType != "evm" {
				continue
			}
			prev, exists := seen[info.ChainID]
			require.False(t, exists, "evm networks %s and %s share chain ID %d", prev, id, info.ChainID)
			seen[info.ChainID] = id
		}
	})

	t.Run("lookup unknown network returns error", func(t *testing.T) {
		_, err := LookupNetwork("nonexistent")
		require.Error(t, err)
	})
}

func TestEVMDerivation(t *testing.T) {
	t.Run("deterministic", func(t *testing.T) {
		a1, err := deriveEVMAddress(testSeed, 60, 0)
		require.NoError(t, err)
		a2, err := deriveEVMAddress(testSeed, 60, 0)
		require.NoError(t, err)
		require.Equal(t, a1, a2)
	})

	t.Run("different indices produce different addresses", func(t *testing.T) {
		a0, err := deriveEVMAddress(testSeed, 60, 0)
		require.NoError(t, err)
		a1, err := deriveEVMAddress(testSeed, 60, 1)
		require.NoError(t, err)
		require.NotEqual(t, a0, a1)
	})

	t.Run("valid checksummed hex format", func(t *testing.T) {
		addr, err := deriveEVMAddress(testSeed, 60, 0)
		require.NoError(t, err)
		require.Len(t, addr, 42)
		require.Equal(t, "0x", addr[:2])
	})
}

func TestSolanaDerivation(t *testing.T) {
	t.Run("deterministic", func(t *testing.T) {
		k1, err := deriveSolanaKey(testSeed, 501, 0)
		require.NoError(t, err)
		k2, err := deriveSolanaKey(testSeed, 501, 0)
		require.NoError(t, err)
		require.Equal(t, k1, k2)
	})

	t.Run("different indices produce different keys", func(t *testing.T) {
		k0, err := deriveSolanaKey(testSeed, 501, 0)
		require.NoError(t, err)
		k1, err := deriveSolanaKey(testSeed, 501, 1)
		require.NoError(t, err)
		require.NotEqual(t, k0, k1)
	})

	t.Run("produces valid base58 address", func(t *testing.T) {
		key, err := deriveSolanaKey(testSeed, 501, 0)
		require.NoError(t, err)
		pub := ed25519.NewKeyFromSeed(key).Public().(ed25519.PublicKey)
		addr := base58.Encode(pub)
		require.GreaterOrEqual(t, len(addr), 32)
		require.LessOrEqual(t, len(addr), 44)
	})
}

func TestCreateMerchantSeed(t *testing.T) {
	kms := mocks.NewMockKMS(t)
	kms.EXPECT().CreateKey(mock.Anything, "kassi-merchant-mer_test123").Return(nil)
	kms.EXPECT().Encrypt(mock.Anything, "kassi-merchant-mer_test123", mock.Anything).
		RunAndReturn(func(_ context.Context, _ string, plaintext []byte) (string, error) {
			return hex.EncodeToString(plaintext), nil
		})

	ciphertext, err := CreateMerchantSeed(t.Context(), kms, "mer_test123")
	require.NoError(t, err)

	seed, err := hex.DecodeString(ciphertext)
	require.NoError(t, err)
	require.Len(t, seed, int(hdkeychain.RecommendedSeedLen))
}

func TestDeriveAddress(t *testing.T) {
	// create a fixed seed for deterministic tests
	fixedSeed := testSeed
	encryptedSeed := hex.EncodeToString(fixedSeed)

	setupKMS := func(t *testing.T) *mocks.MockKMS {
		kms := mocks.NewMockKMS(t)
		kms.EXPECT().Decrypt(mock.Anything, "kassi-merchant-mer_derive", encryptedSeed).
			Return(fixedSeed, nil)
		return kms
	}

	t.Run("evm returns checksummed hex", func(t *testing.T) {
		addr, err := DeriveAddress(t.Context(), setupKMS(t), "mer_derive", encryptedSeed, "ethereum-mainnet", 0)
		require.NoError(t, err)
		require.Len(t, addr, 42)
		require.Equal(t, "0x", addr[:2])
	})

	t.Run("solana returns base58", func(t *testing.T) {
		addr, err := DeriveAddress(t.Context(), setupKMS(t), "mer_derive", encryptedSeed, "solana-mainnet", 0)
		require.NoError(t, err)
		require.GreaterOrEqual(t, len(addr), 32)
		require.LessOrEqual(t, len(addr), 44)
	})

	t.Run("all evm networks derive the same address for same seed and index", func(t *testing.T) {
		var first string
		for id, info := range networks {
			if info.ChainType != "evm" {
				continue
			}
			kms := mocks.NewMockKMS(t)
			kms.EXPECT().Decrypt(mock.Anything, "kassi-merchant-mer_derive", encryptedSeed).
				Return(fixedSeed, nil)

			addr, err := DeriveAddress(t.Context(), kms, "mer_derive", encryptedSeed, id, 0)
			require.NoError(t, err, "network %s", id)
			if first == "" {
				first = addr
			} else {
				require.Equal(t, first, addr, "network %s derived different address", id)
			}
		}
	})

	t.Run("unknown network returns error", func(t *testing.T) {
		kms := mocks.NewMockKMS(t)
		_, err := DeriveAddress(t.Context(), kms, "mer_derive", encryptedSeed, "fake-network", 0)
		require.Error(t, err)
	})
}
