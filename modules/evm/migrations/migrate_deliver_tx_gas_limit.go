package migrations

import (
	"github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

func MigrateDeliverTxHookWasmGasLimitParam(ctx sdk.Context, k *keeper.Keeper) error {
	// Fetch the v11 parameters
	keeperParams := k.GetParamsIfExists(ctx)

	// Add DeliverTxHookWasmGasLimit to with default value
	keeperParams.DeliverTxHookWasmGasLimit = types.DefaultParams().DeliverTxHookWasmGasLimit

	// Set the updated parameters back in the keeper
	k.SetParams(ctx, keeperParams)

	return nil
}
