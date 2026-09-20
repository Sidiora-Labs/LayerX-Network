package keeper_test

import (
	"math"
	"testing"

	"github.com/sidiora-labs/paxeer-network/modules/oracle/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/oracle/keeper/testutils"
	"github.com/sidiora-labs/paxeer-network/modules/oracle/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/stretchr/testify/require"
)

func TestMigrate5To6RejectsInconsistentCountersAtomically(t *testing.T) {
	cases := []struct {
		name      string
		counter   types.VotePenaltyCounter
		malformed bool
	}{
		{name: "miss exceeds elapsed", counter: types.VotePenaltyCounter{MissCount: 26}},
		{name: "combined exceeds elapsed", counter: types.VotePenaltyCounter{MissCount: 12, AbstainCount: 14}},
		{name: "addition overflow", counter: types.VotePenaltyCounter{MissCount: math.MaxUint64, AbstainCount: 1}},
		{name: "abstain overflow", counter: types.VotePenaltyCounter{AbstainCount: math.MaxUint64}},
		{name: "invalid encoding", malformed: true},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			input := testutils.CreateTestInput(t)
			ctx := input.Ctx.WithBlockHeight(25)
			store := ctx.KVStore(input.OracleKeeper.GetStoreKey())
			first := sdk.ValAddress(make([]byte, 20))
			second := sdk.ValAddress(append(make([]byte, 19), 1))
			firstKey := types.GetVotePenaltyCounterKey(first)
			secondKey := types.GetVotePenaltyCounterKey(second)
			firstBytes := input.OracleKeeper.GetCdc().MustMarshal(&types.VotePenaltyCounter{MissCount: 1, AbstainCount: 2})
			secondBytes := input.OracleKeeper.GetCdc().MustMarshal(&tc.counter)
			if tc.malformed {
				secondBytes = []byte{0xff}
			}
			store.Set(firstKey, firstBytes)
			store.Set(secondKey, secondBytes)
			require.Error(t, keeper.NewMigrator(input.OracleKeeper).Migrate5To6(ctx))
			require.Equal(t, firstBytes, store.Get(firstKey))
			require.Equal(t, secondBytes, store.Get(secondKey))
		})
	}
}

func TestMigrate5To6AcceptsZeroSuccessBoundary(t *testing.T) {
	input := testutils.CreateTestInput(t)
	ctx := input.Ctx.WithBlockHeight(25)
	address := testutils.ValAddrs[0]
	input.OracleKeeper.SetVotePenaltyCounter(ctx, address, 12, 13, 0)
	require.NoError(t, keeper.NewMigrator(input.OracleKeeper).Migrate5To6(ctx))
	require.Equal(t, types.VotePenaltyCounter{MissCount: 12, AbstainCount: 13, SuccessCount: 0}, input.OracleKeeper.GetVotePenaltyCounter(ctx, address))
}
