package epoch_test

import (
	"testing"
	"time"

	"github.com/sidiora-labs/paxeer-network/modules/epoch"
	"github.com/sidiora-labs/paxeer-network/modules/epoch/types"
	keepertest "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/sidiora-labs/paxeer-network/testutil/nullify"
	"github.com/stretchr/testify/require"
)

func TestGenesis(t *testing.T) {
	now := time.Now()
	genesisState := types.GenesisState{
		Params: types.DefaultParams(),
		Epoch: &types.Epoch{
			GenesisTime:           now,
			EpochDuration:         time.Minute,
			CurrentEpoch:          1,
			CurrentEpochStartTime: now,
			CurrentEpochHeight:    0,
		},
	}

	k, ctx := keepertest.EpochKeeper(t)
	epoch.InitGenesis(ctx, *k, genesisState)
	got := epoch.ExportGenesis(ctx, *k)
	require.NotNil(t, got)
	require.Equal(t, got.Epoch.CurrentEpoch, genesisState.Epoch.CurrentEpoch)

	nullify.Fill(&genesisState)
	nullify.Fill(got)
}

func TestDefaultGenesisUsesConsensusBlockTime(t *testing.T) {
	k, ctx := keepertest.EpochKeeper(t)
	blockTime := time.Unix(1_700_000_000, 0).UTC()
	ctx = ctx.WithBlockTime(blockTime)

	epoch.InitGenesis(ctx, *k, *types.DefaultGenesis())
	got := k.GetEpoch(ctx)

	require.Equal(t, blockTime, got.GenesisTime)
	require.Equal(t, blockTime, got.CurrentEpochStartTime)
}

func TestDefaultGenesisRejectsMissingConsensusBlockTime(t *testing.T) {
	k, ctx := keepertest.EpochKeeper(t)

	require.Panics(t, func() {
		epoch.InitGenesis(ctx, *k, *types.DefaultGenesis())
	})
}
