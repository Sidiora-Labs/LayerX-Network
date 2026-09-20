package keeper

import (
	"github.com/sidiora-labs/paxeer-network/modules/epoch/types"
)

var _ types.QueryServer = Keeper{}
