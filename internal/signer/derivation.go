package signer

import (
	"crypto/hmac"
	"crypto/sha512"
	"encoding/binary"
	"fmt"

	"github.com/btcsuite/btcd/btcutil/hdkeychain"
	"github.com/btcsuite/btcd/chaincfg"
	"github.com/ethereum/go-ethereum/crypto"
)

// NetworkInfo holds the derivation and signing parameters for a blockchain network.
type NetworkInfo struct {
	ChainType string // "evm" or "solana"
	CoinType  uint32 // SLIP-44 coin type for BIP-44 derivation path
	ChainID   uint64 // EVM chain ID for tx signing (EIP-155), 0 for non-EVM
}

var networks = map[string]NetworkInfo{
	// evm mainnets
	"ethereum-mainnet": {ChainType: "evm", CoinType: 60, ChainID: 1},
	"base-mainnet":     {ChainType: "evm", CoinType: 60, ChainID: 8453},
	"polygon-mainnet":  {ChainType: "evm", CoinType: 60, ChainID: 137},
	"bsc-mainnet":      {ChainType: "evm", CoinType: 60, ChainID: 56},
	"optimism-mainnet": {ChainType: "evm", CoinType: 60, ChainID: 10},
	"arbitrum-mainnet": {ChainType: "evm", CoinType: 60, ChainID: 42161},
	"gnosis-mainnet":   {ChainType: "evm", CoinType: 60, ChainID: 100},

	// evm testnets
	"ethereum-sepolia": {ChainType: "evm", CoinType: 60, ChainID: 11155111},
	"base-sepolia":     {ChainType: "evm", CoinType: 60, ChainID: 84532},
	"bsc-testnet":      {ChainType: "evm", CoinType: 60, ChainID: 97},
	"optimism-sepolia": {ChainType: "evm", CoinType: 60, ChainID: 11155420},
	"arbitrum-sepolia": {ChainType: "evm", CoinType: 60, ChainID: 421614},
	"gnosis-chiado":    {ChainType: "evm", CoinType: 60, ChainID: 10200},

	// solana
	"solana-mainnet": {ChainType: "solana", CoinType: 501},
	"solana-devnet":  {ChainType: "solana", CoinType: 501},
}

// LookupNetwork returns the network info for a given network ID, or an error
// if the network is not supported.
func LookupNetwork(networkID string) (NetworkInfo, error) {
	info, ok := networks[networkID]
	if !ok {
		return NetworkInfo{}, fmt.Errorf("unknown network: %s", networkID)
	}
	return info, nil
}

// deriveEVMKey derives a secp256k1 private key via BIP-32/BIP-44:
// m / 44' / coin_type' / 0' / 0 / index
func deriveEVMKey(seed []byte, coinType uint32, index uint32) (*hdkeychain.ExtendedKey, error) {
	master, err := hdkeychain.NewMaster(seed, &chaincfg.MainNetParams)
	if err != nil {
		return nil, fmt.Errorf("creating master key: %w", err)
	}

	path := []uint32{
		hdkeychain.HardenedKeyStart + 44,
		hdkeychain.HardenedKeyStart + coinType,
		hdkeychain.HardenedKeyStart + 0,
		0,
		index,
	}

	key := master
	for _, segment := range path {
		key, err = key.Derive(segment)
		if err != nil {
			return nil, fmt.Errorf("deriving segment %d: %w", segment, err)
		}
	}

	return key, nil
}

func deriveEVMAddress(seed []byte, coinType uint32, index uint32) (string, error) {
	key, err := deriveEVMKey(seed, coinType, index)
	if err != nil {
		return "", err
	}

	privKey, err := key.ECPrivKey()
	if err != nil {
		return "", fmt.Errorf("extracting EC private key: %w", err)
	}

	return crypto.PubkeyToAddress(privKey.ToECDSA().PublicKey).Hex(), nil
}

// SLIP-0010 ed25519 derivation (all segments hardened):
// m / 44' / coin_type' / 0' / 0' / index'
func deriveSolanaKey(seed []byte, coinType uint32, index uint32) ([]byte, error) {
	key, chainCode, err := slip0010Master(seed)
	if err != nil {
		return nil, err
	}

	path := []uint32{
		0x8000002C,            // 44'
		0x80000000 | coinType, // coin_type'
		0x80000000,            // 0'
		0x80000000,            // 0'
		0x80000000 | index,    // index'
	}

	for _, segment := range path {
		key, chainCode, err = slip0010DeriveChild(key, chainCode, segment)
		if err != nil {
			return nil, err
		}
	}

	return key, nil
}

func slip0010Master(seed []byte) ([]byte, []byte, error) {
	h := hmac.New(sha512.New, []byte("ed25519 seed"))
	if _, err := h.Write(seed); err != nil {
		return nil, nil, fmt.Errorf("computing SLIP-0010 master: %w", err)
	}
	sum := h.Sum(nil)
	return sum[:32], sum[32:], nil
}

func slip0010DeriveChild(key, chainCode []byte, index uint32) ([]byte, []byte, error) {
	// SLIP-0010 ed25519: only hardened derivation supported.
	// data = 0x00 || key || index (big-endian)
	data := make([]byte, 1+32+4)
	data[0] = 0x00
	copy(data[1:33], key)
	binary.BigEndian.PutUint32(data[33:], index)

	h := hmac.New(sha512.New, chainCode)
	if _, err := h.Write(data); err != nil {
		return nil, nil, fmt.Errorf("deriving SLIP-0010 child: %w", err)
	}
	sum := h.Sum(nil)
	return sum[:32], sum[32:], nil
}
