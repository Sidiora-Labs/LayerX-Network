package types

import (
	"crypto/sha256"
	"math/big"

	"github.com/ethereum/go-ethereum/accounts/abi"
	"github.com/ethereum/go-ethereum/common"
)

const (
	// ExchangeAddress is the layerxExchange precompile: the address every
	// exchange EVM log is emitted from and the `address(this)` of every
	// intent identifier.
	ExchangeAddress = "0x0000000000000000000000000000000000001015"

	IntentDomain = "LXP/Paxeer/exchange-intent/v1"
)

func mustType(name string) abi.Type {
	t, err := abi.NewType(name, "", nil)
	if err != nil {
		panic(err)
	}
	return t
}

var intentArguments = abi.Arguments{{Type: mustType("string")}, {Type: mustType("uint256")},
	{Type: mustType("address")}, {Type: mustType("address")}, {Type: mustType("uint8")}, {Type: mustType("uint64")}}

// IntentID is sha256(abi.encode(domain, chainid, this, owner, kind, nonce))
// with the exchange precompile as address(this). nonce is the owner's
// 1-based intent counter, shared by every kind.
func IntentID(chainID *big.Int, owner common.Address, kind IntentKind, nonce uint64) [32]byte {
	packed, err := intentArguments.Pack(IntentDomain, chainID, common.HexToAddress(ExchangeAddress), owner,
		uint8(kind), nonce) //nolint:gosec
	if err != nil {
		panic(err)
	}
	return sha256.Sum256(packed)
}
