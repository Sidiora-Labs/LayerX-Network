package types

import (
	"encoding/binary"

	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	authtypes "github.com/sidiora-labs/paxeer-network/sdk/x/auth/types"
)

const (
	ModuleName   = "xweb"
	StoreKey     = ModuleName
	RouterKey    = ModuleName
	QuerierRoute = ModuleName

	// PrecompileAddress is the xweb precompile address.
	PrecompileAddress = "0x0000000000000000000000000000000000001019"

	// MaxAttestors bounds the attestor set and so the signatures one fulfil
	// may carry.
	MaxAttestors = 64

	// MaxResponseBytes bounds the response content one result stores.
	MaxResponseBytes = 4096
)

var (
	ParamsKey      = []byte{0x01}
	AttestorSetKey = []byte{0x02}
	PausedKey      = []byte{0x03}
	NonceKey       = []byte{0x04}
	RequestPrefix  = []byte{0x05}
	ResultPrefix   = []byte{0x06}
	xwebModuleAddr = authtypes.NewModuleAddress(ModuleName)
)

// ModuleAddress is the xweb module account. It holds the fee of every pending
// request until a fulfilment pays it to the signers or a refund returns it.
func ModuleAddress() sdk.AccAddress { return xwebModuleAddr }

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

func RequestKey(id uint64) []byte { return join(RequestPrefix, u64(id)) }

func ResultKey(id uint64) []byte { return join(ResultPrefix, u64(id)) }
