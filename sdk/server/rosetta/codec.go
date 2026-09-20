package rosetta

import (
	"github.com/sidiora-labs/paxeer-network/sdk/codec"
	codectypes "github.com/sidiora-labs/paxeer-network/sdk/codec/types"
	cryptocodec "github.com/sidiora-labs/paxeer-network/sdk/crypto/codec"
	authcodec "github.com/sidiora-labs/paxeer-network/sdk/x/auth/types"
	bankcodec "github.com/sidiora-labs/paxeer-network/sdk/x/bank/types"
)

// MakeCodec generates the codec required to interact
// with the cosmos APIs used by the rosetta gateway
func MakeCodec() (*codec.ProtoCodec, codectypes.InterfaceRegistry) {
	ir := codectypes.NewInterfaceRegistry()
	cdc := codec.NewProtoCodec(ir)

	authcodec.RegisterInterfaces(ir)
	bankcodec.RegisterInterfaces(ir)
	cryptocodec.RegisterInterfaces(ir)

	return cdc, ir
}
