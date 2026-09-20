package migrations_test

import (
	"testing"

	tmtypes "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/types"
	"github.com/sidiora-labs/paxeer-network/modules/evm/migrations"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

func TestMigrateDeliverTxHookWasmGasLimitParam(t *testing.T) {
	k := testkeeper.EVMTestApp.EvmKeeper
	ctx := testkeeper.EVMTestApp.NewContext(false, tmtypes.Header{})

	currParams := k.GetParams(ctx)

	// Keep a copy of the other parameters to compare later
	priorityNormalizer := currParams.PriorityNormalizer
	baseFeePerGas := currParams.BaseFeePerGas
	minimumFeePerGas := currParams.MinimumFeePerGas

	// Perform the migration
	err := migrations.MigrateDeliverTxHookWasmGasLimitParam(ctx, &k)
	require.NoError(t, err)

	keeperParams := k.GetParams(ctx)

	// Ensure that the DeliverTxHookWasmGasLimit was migrated to the default value
	require.Equal(t, keeperParams.GetDeliverTxHookWasmGasLimit(), types.DefaultParams().DeliverTxHookWasmGasLimit)

	// Verify that the other parameters were not changed by the migration
	require.True(t, keeperParams.PriorityNormalizer.Equal(priorityNormalizer))
	require.True(t, keeperParams.BaseFeePerGas.Equal(baseFeePerGas))
	require.True(t, keeperParams.MinimumFeePerGas.Equal(minimumFeePerGas))
}
