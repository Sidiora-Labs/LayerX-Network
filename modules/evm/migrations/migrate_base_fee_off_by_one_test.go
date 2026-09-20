package migrations_test

import (
	"testing"

	"github.com/sidiora-labs/paxeer-network/modules/evm/migrations"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

func TestMigrateBaseFeeOffByOne(t *testing.T) {
	k := testkeeper.EVMTestApp.EvmKeeper
	ctx := testkeeper.EVMTestApp.GetContextForDeliverTx([]byte{}).WithBlockHeight(8)
	bf := sdk.NewDec(100)
	k.SetCurrBaseFeePerGas(ctx, bf)
	require.Equal(t, k.GetMinimumFeePerGas(ctx), k.GetNextBaseFeePerGas(ctx))
	// do the migration
	require.Nil(t, migrations.MigrateBaseFeeOffByOne(ctx, &k))
	require.Equal(t, bf, k.GetNextBaseFeePerGas(ctx))
	require.Equal(t, bf, k.GetCurrBaseFeePerGas(ctx))
}
