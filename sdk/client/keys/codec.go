package keys

import (
	"github.com/sidiora-labs/paxeer-network/sdk/codec"
	cryptocodec "github.com/sidiora-labs/paxeer-network/sdk/crypto/codec"
)

// TODO: remove this file https://github.com/cosmos/cosmos-sdk/issues/8047

// KeysCdc defines codec to be used with key operations
var KeysCdc *codec.LegacyAmino

func init() {
	KeysCdc = codec.NewLegacyAmino()
	cryptocodec.RegisterCrypto(KeysCdc)
	KeysCdc.Seal()
}

// marshal keys
func MarshalJSON(o interface{}) ([]byte, error) {
	return KeysCdc.MarshalAsJSON(o)
}

// unmarshal json
func UnmarshalJSON(bz []byte, ptr interface{}) error {
	return KeysCdc.UnmarshalAsJSON(bz, ptr)
}
