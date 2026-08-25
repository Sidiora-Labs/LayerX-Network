package keeper_test

import (
	"testing"

	"github.com/sidiora-labs/paxeer-network/modules/epoch/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

func TestEpochQuery(t *testing.T) {
	keeper, ctx := testkeeper.EpochKeeper(t)
	wctx := sdk.WrapSDKContext(ctx)
	epoch := types.DefaultEpoch()
	keeper.SetEpoch(ctx, epoch)

	response, err := keeper.Epoch(wctx, &types.QueryEpochRequest{})
	require.NoError(t, err)
	require.Equal(t, &types.QueryEpochResponse{Epoch: epoch}, response)
}
