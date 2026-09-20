package localhost

import (
	"github.com/sidiora-labs/paxeer-network/interchain/modules/light-clients/09-localhost/types"
)

// Name returns the IBC client name
func Name() string {
	return types.SubModuleName
}
