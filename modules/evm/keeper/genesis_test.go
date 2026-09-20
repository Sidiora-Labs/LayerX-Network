package keeper_test

import (
	"bytes"
	"testing"

	"github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

func TestInitGenesis(t *testing.T) {
	k := &testkeeper.EVMTestApp.EvmKeeper
	ctx := testkeeper.EVMTestApp.GetContextForDeliverTx([]byte{})
	// coinbase address must be associated
	coinbasePaxAddr, associated := k.GetPaxAddress(ctx, keeper.GetCoinbaseAddress())
	require.True(t, associated)
	require.True(t, bytes.Equal(coinbasePaxAddr, k.AccountKeeper().GetModuleAddress("fee_collector")))
}
