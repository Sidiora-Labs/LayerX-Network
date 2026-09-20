package keeper

import (
	"github.com/sidiora-labs/paxeer-network/modules/epoch/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

func (k Keeper) AfterEpochEnd(ctx sdk.Context, epoch types.Epoch) {
	k.hooks.AfterEpochEnd(ctx, epoch)
}

func (k Keeper) BeforeEpochStart(ctx sdk.Context, epoch types.Epoch) {
	k.hooks.BeforeEpochStart(ctx, epoch)
}
