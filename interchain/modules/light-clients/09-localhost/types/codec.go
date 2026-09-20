package types

import (
	codectypes "github.com/sidiora-labs/paxeer-network/sdk/codec/types"

	"github.com/sidiora-labs/paxeer-network/interchain/modules/core/exported"
)

// RegisterInterfaces register the ibc interfaces submodule implementations to protobuf
// Any.
func RegisterInterfaces(registry codectypes.InterfaceRegistry) {
	registry.RegisterImplementations(
		(*exported.ClientState)(nil),
		&ClientState{},
	)
}
