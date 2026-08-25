package tendermint

import (
	"github.com/sidiora-labs/paxeer-network/interchain/modules/light-clients/07-tendermint/types"
)

// Name returns the IBC client name
func Name() string {
	return types.SubModuleName
}
