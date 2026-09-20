package migrations

import (
	"github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

func MigrateEip1559Params(ctx sdk.Context, k *keeper.Keeper) error {
	keeperParams := k.GetParamsIfExists(ctx)
	keeperParams.MaxDynamicBaseFeeUpwardAdjustment = types.DefaultParams().MaxDynamicBaseFeeUpwardAdjustment
	keeperParams.MaxDynamicBaseFeeDownwardAdjustment = types.DefaultParams().MaxDynamicBaseFeeDownwardAdjustment
	keeperParams.TargetGasUsedPerBlock = types.DefaultParams().TargetGasUsedPerBlock
	keeperParams.MinimumFeePerGas = types.DefaultParams().MinimumFeePerGas
	k.SetParams(ctx, keeperParams)
	return nil
}
