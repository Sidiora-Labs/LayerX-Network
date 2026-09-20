package epoch

import (
	"github.com/sidiora-labs/paxeer-network/modules/epoch/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/epoch/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// InitGenesis initializes the capability module's state from a provided genesis
// state.
func InitGenesis(ctx sdk.Context, k keeper.Keeper, genState types.GenesisState) {
	epoch := *genState.Epoch
	if epoch.GenesisTime.Equal(types.DefaultGenesisTime()) && epoch.CurrentEpochStartTime.Equal(types.DefaultGenesisTime()) {
		if ctx.BlockTime().IsZero() {
			panic("epoch default genesis requires a non-zero consensus block time")
		}
		epoch.GenesisTime = ctx.BlockTime()
		epoch.CurrentEpochStartTime = ctx.BlockTime()
	}
	// this line is used by starport scaffolding # genesis/module/init
	k.SetParams(ctx, genState.Params)
	k.SetEpoch(ctx, epoch)
}

// ExportGenesis returns the capability module's exported genesis.
func ExportGenesis(ctx sdk.Context, k keeper.Keeper) *types.GenesisState {
	genesis := types.DefaultGenesis()
	genesis.Params = k.GetParams(ctx)
	epoch := k.GetEpoch(ctx)
	genesis.Epoch = &epoch

	return genesis
}
