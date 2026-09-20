package migrations

import (
	"github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

func MigrateBaseFeeOffByOne(ctx sdk.Context, k *keeper.Keeper) error {
	baseFee := k.GetCurrBaseFeePerGas(ctx)
	k.SetNextBaseFeePerGas(ctx, baseFee)
	return nil
}
