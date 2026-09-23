package types

import (
	"encoding/binary"
)

const (
	// ModuleName is the module name and the name of the account that escrows
	// every market's token supply, real quote balance and undistributed fees.
	ModuleName = "launchpad"

	// TreasuryName is the module account that receives creation fees and the
	// protocol share of buy fees.
	TreasuryName = "launchpad_treasury"

	// StoreKey is the primary module store key.
	StoreKey = ModuleName

	// RouterKey is the message route.
	RouterKey = ModuleName

	// QuerierRoute is the query routing key.
	QuerierRoute = ModuleName

	// LaunchpadAddress is the EVM address of the launchpad precompile.
	LaunchpadAddress = "0x0000000000000000000000000000000000001017"

	// DeadAddress is FeeAccumulator's burn destination.
	DeadAddress = "0x000000000000000000000000000000000000dEaD"

	// TokenDecimals is the exponent of every launched token's display unit.
	TokenDecimals = 6
)

var (
	ParamsKey              = []byte{0x01}
	MarketCountKey         = []byte{0x02}
	ProtocolFeesPendingKey = []byte{0x03}
	MarketPrefix           = []byte{0x10}
	MarketIndexPrefix      = []byte{0x11}
	MarketPointerPrefix    = []byte{0x12}
	CreatorMarketPrefix    = []byte{0x13}
	AirdropEpochPrefix     = []byte{0x20}
	AirdropClaimPrefix     = []byte{0x21}
)

func join(prefix []byte, parts ...[]byte) []byte {
	out := append([]byte(nil), prefix...)
	for _, part := range parts {
		out = append(out, part...)
	}
	return out
}

func lengthPrefixed(value []byte) []byte {
	return append([]byte{byte(len(value))}, value...)
}

func uint64Bytes(value uint64) []byte { return binary.BigEndian.AppendUint64(nil, value) }

func MarketKey(denom string) []byte            { return join(MarketPrefix, []byte(denom)) }
func MarketIndexKey(index uint64) []byte       { return join(MarketIndexPrefix, uint64Bytes(index)) }
func MarketPointerKey(pointer [20]byte) []byte { return join(MarketPointerPrefix, pointer[:]) }

// CreatorMarketPrefixFor is the prefix of one creator's market index.
func CreatorMarketPrefixFor(creator []byte) []byte {
	return join(CreatorMarketPrefix, lengthPrefixed(creator))
}

func CreatorMarketKey(creator []byte, index uint64) []byte {
	return join(CreatorMarketPrefixFor(creator), uint64Bytes(index))
}

func AirdropEpochKey(denom string, epoch uint64) []byte {
	return join(AirdropEpochPrefix, lengthPrefixed([]byte(denom)), uint64Bytes(epoch))
}

func AirdropClaimKey(denom string, holder []byte, epoch uint64) []byte {
	return join(AirdropClaimPrefix, lengthPrefixed([]byte(denom)), lengthPrefixed(holder), uint64Bytes(epoch))
}

// ParseAirdropClaimKey splits an AirdropClaimKey into its denom, holder and
// epoch.
func ParseAirdropClaimKey(key []byte) (string, []byte, uint64, bool) {
	rest := key[len(AirdropClaimPrefix):]
	if len(rest) < 1 || len(rest) < 1+int(rest[0]) {
		return "", nil, 0, false
	}
	denom := string(rest[1 : 1+int(rest[0])])
	rest = rest[1+int(rest[0]):]
	if len(rest) < 1 || len(rest) != 1+int(rest[0])+8 {
		return "", nil, 0, false
	}
	holder := append([]byte(nil), rest[1:1+int(rest[0])]...)
	return denom, holder, binary.BigEndian.Uint64(rest[1+int(rest[0]):]), true
}
