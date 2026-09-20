package migrations

import (
	"github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

func MigrateRemoveCurrBlockBaseFee(ctx sdk.Context, k *keeper.Keeper) error {
	currBlockBaseFee := k.GetCurrBaseFeePerGas(ctx)
	k.SetNextBaseFeePerGas(ctx, currBlockBaseFee)
	// just store min base fee in curr block base fee
	k.SetCurrBaseFeePerGas(ctx, types.DefaultMinFeePerGas)
	return nil
}
