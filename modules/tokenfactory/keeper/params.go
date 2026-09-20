package keeper

import (
	"github.com/sidiora-labs/paxeer-network/modules/tokenfactory/types"

	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// GetParams returns the total set params.
func (k Keeper) GetParams(ctx sdk.Context) (params types.Params) {
	k.paramSpace.GetParamSet(ctx, &params)
	return params
}

// SetParams sets the total set of params.
func (k Keeper) SetParams(ctx sdk.Context, params types.Params) {
	k.paramSpace.SetParamSet(ctx, &params)
}
