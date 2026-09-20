package keeper

import (
	"context"

	"github.com/sidiora-labs/paxeer-network/modules/epoch/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

func (k Keeper) Epoch(c context.Context, _ *types.QueryEpochRequest) (*types.QueryEpochResponse, error) {
	ctx := sdk.UnwrapSDKContext(c)

	epoch := k.GetEpoch(ctx)
	return &types.QueryEpochResponse{Epoch: epoch}, nil
}
