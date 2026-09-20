package keeper

import (
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/sdk/x/auth/types"
)

// SetParams sets the auth module's parameters.
func (ak AccountKeeper) SetParams(ctx sdk.Context, params types.Params) {
	ak.paramSubspace.SetParamSet(ctx, &params)
}

// GetParams gets the auth module's parameters.
func (ak AccountKeeper) GetParams(ctx sdk.Context) (params types.Params) {
	ak.paramSubspace.GetParamSet(ctx, &params)
	return
}
