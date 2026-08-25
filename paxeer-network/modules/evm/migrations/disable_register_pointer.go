package migrations

import (
	"github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

func MigrateDisableRegisterPointer(ctx sdk.Context, k *keeper.Keeper) error {
	params := k.GetParams(ctx)
	params.RegisterPointerDisabled = true
	k.SetParams(ctx, params)
	return nil
}
