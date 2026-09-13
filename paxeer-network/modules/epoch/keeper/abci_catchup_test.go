package keeper_test

import (
	"testing"
	"time"

	tmproto "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/types"
	"github.com/sidiora-labs/paxeer-network/modules/epoch/types"
	"github.com/sidiora-labs/paxeer-network/node"
	"github.com/stretchr/testify/require"
)

func TestEpochCatchupPreservesElapsedPeriods(t *testing.T) {
	application := app.Setup(t, false, false, false)
	start := time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)
	duration := time.Minute
	blockTime := start.Add(5*duration + time.Second)
	ctx := application.BaseApp.NewContext(false, tmproto.Header{Time: blockTime, Height: 10})
	initial := types.Epoch{GenesisTime: start, EpochDuration: duration, CurrentEpochStartTime: start, CurrentEpochHeight: 1, CurrentEpoch: 20}
	application.EpochKeeper.SetEpoch(ctx, initial)
	for offset := uint64(1); offset <= 5; offset++ {
		ctx = ctx.WithBlockHeight(10 + int64(offset))
		application.EpochKeeper.BeginBlock(ctx)
		current := application.EpochKeeper.GetEpoch(ctx)
		require.Equal(t, initial.CurrentEpoch+offset, current.CurrentEpoch)
		require.Equal(t, start.Add(time.Duration(offset)*duration), current.CurrentEpochStartTime)
		require.Equal(t, ctx.BlockHeight(), current.CurrentEpochHeight)
		require.Equal(t, start, current.GenesisTime)
	}
	caughtUp := application.EpochKeeper.GetEpoch(ctx)
	application.EpochKeeper.BeginBlock(ctx.WithBlockHeight(16))
	require.Equal(t, caughtUp, application.EpochKeeper.GetEpoch(ctx))
}
