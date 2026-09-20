package keeper

import (
	"github.com/sidiora-labs/paxeer-network/modules/epoch/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// GetParams get all parameters as types.Params
func (k Keeper) GetParams(_ sdk.Context) types.Params {
	return types.NewParams()
}

// SetParams set the params
func (k Keeper) SetParams(ctx sdk.Context, params types.Params) {
	k.paramstore.SetParamSet(ctx, &params)
}
