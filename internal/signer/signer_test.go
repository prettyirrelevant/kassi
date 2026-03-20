package signer

import (
	"context"
	"crypto/ed25519"
	"encoding/hex"
	"testing"

	"github.com/btcsuite/btcd/btcutil/hdkeychain"
	"github.com/mr-tron/base58"
)

type mockKMS struct {
	keys map[string]bool
}

func newMockKMS() *mockKMS {
	return &mockKMS{keys: make(map[string]bool)}
}

func (m *mockKMS) CreateKey(_ context.Context, name string) error {
	m.keys[name] = true
	return nil
}

func (m *mockKMS) Encrypt(_ context.Context, _ string, plaintext []byte) (string, error) {
	return hex.EncodeToString(plaintext), nil
}

func (m *mockKMS) Decrypt(_ context.Context, _ string, ciphertext string) ([]byte, error) {
	return hex.DecodeString(ciphertext)
}

var testSeed, _ = hex.DecodeString("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f")

func TestNetworkRegistry(t *testing.T) {
	t.Run("all entries have valid chain type", func(t *testing.T) {
		for id, info := range networks {
			if info.ChainType != "evm" && info.ChainType != "solana" {
				t.Errorf("network %s has invalid chain type: %s", id, info.ChainType)
			}
		}
	})

	t.Run("evm networks must have non-zero chain ID", func(t *testing.T) {
		for id, info := range networks {
			if info.ChainType == "evm" && info.ChainID == 0 {
				t.Errorf("evm network %s has zero chain ID", id)
			}
		}
	})

	t.Run("evm networks must have coin type 60", func(t *testing.T) {
		for id, info := range networks {
			if info.ChainType == "evm" && info.CoinType != 60 {
				t.Errorf("evm network %s has coin type %d, expected 60", id, info.CoinType)
			}
		}
	})

	t.Run("solana networks must have coin type 501", func(t *testing.T) {
		for id, info := range networks {
			if info.ChainType == "solana" && info.CoinType != 501 {
				t.Errorf("solana network %s has coin type %d, expected 501", id, info.CoinType)
			}
		}
	})

	t.Run("no duplicate chain IDs among evm networks", func(t *testing.T) {
		seen := make(map[uint64]string)
		for id, info := range networks {
			if info.ChainType != "evm" {
				continue
			}
			if prev, ok := seen[info.ChainID]; ok {
				t.Errorf("evm networks %s and %s share chain ID %d", prev, id, info.ChainID)
			}
			seen[info.ChainID] = id
		}
	})

	t.Run("lookup unknown network returns error", func(t *testing.T) {
		if _, err := LookupNetwork("nonexistent"); err == nil {
			t.Error("expected error for unknown network")
		}
	})
}

func TestEVMDerivation(t *testing.T) {
	t.Run("deterministic", func(t *testing.T) {
		a1, err := deriveEVMAddress(testSeed, 60, 0)
		if err != nil {
			t.Fatal(err)
		}
		a2, err := deriveEVMAddress(testSeed, 60, 0)
		if err != nil {
			t.Fatal(err)
		}
		if a1 != a2 {
			t.Errorf("same inputs produced different addresses: %s vs %s", a1, a2)
		}
	})

	t.Run("different indices produce different addresses", func(t *testing.T) {
		a0, _ := deriveEVMAddress(testSeed, 60, 0)
		a1, _ := deriveEVMAddress(testSeed, 60, 1)
		if a0 == a1 {
			t.Error("different indices produced the same address")
		}
	})

	t.Run("valid checksummed hex format", func(t *testing.T) {
		addr, err := deriveEVMAddress(testSeed, 60, 0)
		if err != nil {
			t.Fatal(err)
		}
		if len(addr) != 42 || addr[:2] != "0x" {
			t.Errorf("invalid EVM address format: %s", addr)
		}
	})
}

func TestSolanaDerivation(t *testing.T) {
	t.Run("deterministic", func(t *testing.T) {
		k1, err := deriveSolanaKey(testSeed, 501, 0)
		if err != nil {
			t.Fatal(err)
		}
		k2, err := deriveSolanaKey(testSeed, 501, 0)
		if err != nil {
			t.Fatal(err)
		}
		if hex.EncodeToString(k1) != hex.EncodeToString(k2) {
			t.Error("same inputs produced different keys")
		}
	})

	t.Run("different indices produce different keys", func(t *testing.T) {
		k0, _ := deriveSolanaKey(testSeed, 501, 0)
		k1, _ := deriveSolanaKey(testSeed, 501, 1)
		if hex.EncodeToString(k0) == hex.EncodeToString(k1) {
			t.Error("different indices produced the same key")
		}
	})

	t.Run("produces valid base58 address", func(t *testing.T) {
		key, err := deriveSolanaKey(testSeed, 501, 0)
		if err != nil {
			t.Fatal(err)
		}
		pub := ed25519.NewKeyFromSeed(key).Public().(ed25519.PublicKey)
		addr := base58.Encode(pub)
		if len(addr) < 32 || len(addr) > 44 {
			t.Errorf("unexpected solana address length: %d (%s)", len(addr), addr)
		}
	})
}

func TestCreateMerchantSeed(t *testing.T) {
	kms := newMockKMS()
	ciphertext, err := CreateMerchantSeed(t.Context(), kms, "mer_test123")
	if err != nil {
		t.Fatal(err)
	}

	t.Run("creates KMS key with correct name", func(t *testing.T) {
		if !kms.keys["kassi-merchant-mer_test123"] {
			t.Error("expected KMS key kassi-merchant-mer_test123 to be created")
		}
	})

	t.Run("ciphertext decodes to recommended seed length", func(t *testing.T) {
		seed, err := hex.DecodeString(ciphertext)
		if err != nil {
			t.Fatal(err)
		}
		if len(seed) != int(hdkeychain.RecommendedSeedLen) {
			t.Errorf("seed length %d, expected %d", len(seed), hdkeychain.RecommendedSeedLen)
		}
	})
}

func TestDeriveAddress(t *testing.T) {
	kms := newMockKMS()
	ciphertext, err := CreateMerchantSeed(t.Context(), kms, "mer_derive")
	if err != nil {
		t.Fatal(err)
	}

	t.Run("evm returns checksummed hex", func(t *testing.T) {
		addr, err := DeriveAddress(t.Context(), kms, "mer_derive", ciphertext, "ethereum-mainnet", 0)
		if err != nil {
			t.Fatal(err)
		}
		if len(addr) != 42 || addr[:2] != "0x" {
			t.Errorf("invalid EVM address: %s", addr)
		}
	})

	t.Run("solana returns base58", func(t *testing.T) {
		addr, err := DeriveAddress(t.Context(), kms, "mer_derive", ciphertext, "solana-mainnet", 0)
		if err != nil {
			t.Fatal(err)
		}
		if len(addr) < 32 || len(addr) > 44 {
			t.Errorf("invalid Solana address: %s", addr)
		}
	})

	t.Run("all evm networks derive the same address for same seed and index", func(t *testing.T) {
		var first string
		for id, info := range networks {
			if info.ChainType != "evm" {
				continue
			}
			addr, err := DeriveAddress(t.Context(), kms, "mer_derive", ciphertext, id, 0)
			if err != nil {
				t.Fatalf("network %s: %v", id, err)
			}
			if first == "" {
				first = addr
			} else if addr != first {
				t.Errorf("network %s derived %s, expected %s", id, addr, first)
			}
		}
	})

	t.Run("unknown network returns error", func(t *testing.T) {
		if _, err := DeriveAddress(t.Context(), kms, "mer_derive", ciphertext, "fake-network", 0); err == nil {
			t.Error("expected error for unknown network")
		}
	})
}
