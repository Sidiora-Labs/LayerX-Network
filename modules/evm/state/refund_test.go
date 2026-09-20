package state_test

import (
	"testing"
	"time"

	"github.com/sidiora-labs/paxeer-network/modules/evm/state"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

func TestGasRefund(t *testing.T) {
	k := &testkeeper.EVMTestApp.EvmKeeper
	ctx := testkeeper.EVMTestApp.GetContextForDeliverTx([]byte{}).WithBlockTime(time.Now())
	statedb := state.NewDBImpl(ctx, k, false)

	require.Equal(t, uint64(0), statedb.GetRefund())
	statedb.AddRefund(2)
	require.Equal(t, uint64(2), statedb.GetRefund())
	statedb.SubRefund(1)
	require.Equal(t, uint64(1), statedb.GetRefund())
	require.Panics(t, func() { statedb.SubRefund(2) })
}
