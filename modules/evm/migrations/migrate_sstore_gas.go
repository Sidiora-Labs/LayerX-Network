package migrations

import (
	"github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// MigrateSstoreGas updates the PaxSstoreSetGasEip2200 parameter to the default value.
func MigrateSstoreGas(ctx sdk.Context, k *keeper.Keeper) error {
	params := k.GetParams(ctx)
	params.PaxSstoreSetGasEip2200 = types.DefaultPaxSstoreSetGasEIP2200
	k.SetParams(ctx, params)
	return nil
}
