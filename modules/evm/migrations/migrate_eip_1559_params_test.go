package migrations_test

import (
	"testing"

	tmtypes "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/types"
	"github.com/sidiora-labs/paxeer-network/modules/evm/migrations"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

func TestMigrateEip1559Params(t *testing.T) {
	k := testkeeper.EVMTestApp.EvmKeeper
	ctx := testkeeper.EVMTestApp.NewContext(false, tmtypes.Header{})

	keeperParams := k.GetParams(ctx)
	keeperParams.BaseFeePerGas = sdk.NewDec(123)

	// Perform the migration
	err := migrations.MigrateEip1559Params(ctx, &k)
	require.NoError(t, err)

	// Ensure that the new EIP-1559 parameters were migrated and the old ones were not changed
	require.Equal(t, keeperParams.BaseFeePerGas, sdk.NewDec(123))
	require.Equal(t, keeperParams.MaxDynamicBaseFeeUpwardAdjustment, types.DefaultParams().MaxDynamicBaseFeeUpwardAdjustment)
	require.Equal(t, keeperParams.MaxDynamicBaseFeeDownwardAdjustment, types.DefaultParams().MaxDynamicBaseFeeDownwardAdjustment)
	require.Equal(t, keeperParams.TargetGasUsedPerBlock, types.DefaultParams().TargetGasUsedPerBlock)
	require.Equal(t, keeperParams.MinimumFeePerGas, types.DefaultParams().MinimumFeePerGas)
}
