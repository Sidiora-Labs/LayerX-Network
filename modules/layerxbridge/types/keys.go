package types

import (
	"encoding/binary"

	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	authtypes "github.com/sidiora-labs/paxeer-network/sdk/x/auth/types"
)

const (
	ModuleName   = "layerxbridge"
	StoreKey     = ModuleName
	RouterKey    = ModuleName
	QuerierRoute = ModuleName

	// BridgeAddress is the layerxBridge precompile address.
	BridgeAddress = "0x0000000000000000000000000000000000001016"

	// MaxAttestors bounds the attestor set and so the signatures one bridgeIn
	// may carry.
	MaxAttestors = 64
)

var (
	ParamsKey        = []byte{0x01}
	ChainPrefix      = []byte{0x02}
	AttestorSetKey   = []byte{0x03}
	CapPrefix        = []byte{0x04}
	PausedKey        = []byte{0x05}
	NullifierPrefix  = []byte{0x06}
	AssetPrefix      = []byte{0x07}
	DenomPrefix      = []byte{0x08}
	OutboundNonceKey = []byte{0x09}
	InFlightPrefix   = []byte{0x0a}
	bridgeModuleAddr = authtypes.NewModuleAddress(ModuleName)
)

// ModuleAddress is the bridge module account. It is the tokenfactory admin of
// every bridged denom and holds a minted or returned amount only for the
// duration of one bridgeIn or bridgeOut.
func ModuleAddress() sdk.AccAddress { return bridgeModuleAddr }

func u64(value uint64) []byte {
	out := make([]byte, 8)
	binary.BigEndian.PutUint64(out, value)
	return out
}

func join(parts ...[]byte) []byte {
	var out []byte
	for _, part := range parts {
		out = append(out, part...)
	}
	return out
}

func ChainKey(chainID uint64) []byte { return join(ChainPrefix, u64(chainID)) }

func CapKey(denom string) []byte { return join(CapPrefix, []byte(denom)) }

func InFlightKey(denom string) []byte { return join(InFlightPrefix, []byte(denom)) }

// AssetKey maps (chain, remote asset) to its bridged asset record.
func AssetKey(chainID uint64, asset Address20) []byte {
	return join(AssetPrefix, u64(chainID), asset[:])
}

// DenomKey maps a bridged denom back to its asset record.
func DenomKey(denom string) []byte { return join(DenomPrefix, []byte(denom)) }

// NullifierKey is unique per remote event: (chain, txHash, logIndex).
func NullifierKey(n Nullifier) []byte {
	return join(NullifierPrefix, u64(n.ChainID), n.TxHash[:], u64(n.LogIndex))
}

func OutboundNonceChainKey(chainID uint64) []byte { return join(OutboundNonceKey, u64(chainID)) }
