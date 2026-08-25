package solomachine

import (
	"github.com/sidiora-labs/paxeer-network/interchain/modules/light-clients/06-solomachine/types"
)

// Name returns the solo machine client name.
func Name() string {
	return types.SubModuleName
}
