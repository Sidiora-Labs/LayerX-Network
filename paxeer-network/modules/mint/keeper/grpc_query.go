package keeper

import (
	"context"

	"github.com/sidiora-labs/paxeer-network/modules/mint/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

var _ types.QueryServer = Querier{}

// Querier defines a wrapper around the modules/mint keeper providing gRPC method
// handlers.
type Querier struct {
	Keeper
}

func NewQuerier(k Keeper) Querier {
	return Querier{Keeper: k}
}

// Params returns params of the mint module.
func (q Querier) Params(c context.Context, _ *types.QueryParamsRequest) (*types.QueryParamsResponse, error) {
	ctx := sdk.UnwrapSDKContext(c)
	params := q.GetParams(ctx)

	return &types.QueryParamsResponse{Params: params}, nil
}

// Returns the most last mint state
func (q Querier) Minter(c context.Context, _ *types.QueryMinterRequest) (*types.QueryMinterResponse, error) {
	ctx := sdk.UnwrapSDKContext(c)
	minter := q.GetMinter(ctx)
	response := types.QueryMinterResponse(minter)
	return &response, nil
}
