package signer

import (
	"crypto/ecdsa"
	"fmt"
	"math/big"
	"strings"

	"github.com/ethereum/go-ethereum/accounts/abi"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
)

// Multicall3Address is the canonical Multicall3 contract deployed on all supported EVM chains.
var Multicall3Address = common.HexToAddress("0xcA11bde05977b3631167028862bE2a173976CA11")

var (
	erc20ABI, _ = abi.JSON(strings.NewReader(
		`[{"name":"transfer","type":"function","inputs":[{"name":"to","type":"address"},{"name":"amount","type":"uint256"}]}]`,
	))
	multicallABI, _ = abi.JSON(strings.NewReader(
		`[{"name":"tryAggregate","type":"function","inputs":[{"name":"requireSuccess","type":"bool"},{"name":"calls","type":"tuple[]","components":[{"name":"target","type":"address"},{"name":"callData","type":"bytes"}]}]}]`,
	))
)

// Multicall3Call represents a single call in a Multicall3 tryAggregate batch.
type Multicall3Call struct {
	Target   common.Address `abi:"target"`
	CallData []byte         `abi:"callData"`
}

func signEVMTx(privKey *ecdsa.PrivateKey, chainID *big.Int, tx *types.Transaction) ([]byte, error) {
	signed, err := types.SignTx(tx, types.LatestSignerForChainID(chainID), privKey)
	if err != nil {
		return nil, fmt.Errorf("signing EVM transaction: %w", err)
	}
	return signed.MarshalBinary()
}

// EncodeERC20Transfer encodes an ERC-20 transfer(address,uint256) call.
func EncodeERC20Transfer(to common.Address, amount *big.Int) ([]byte, error) {
	return erc20ABI.Pack("transfer", to, amount)
}

// EncodeMulticall3 encodes a Multicall3 tryAggregate(true, calls) call.
func EncodeMulticall3(calls []Multicall3Call) ([]byte, error) {
	return multicallABI.Pack("tryAggregate", true, calls)
}
