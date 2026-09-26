package keeper

import (
	"github.com/sidiora-labs/paxeer-network/modules/xweb/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// InitGenesis loads a validated genesis.
func (k Keeper) InitGenesis(ctx sdk.Context, genesis types.GenesisState) {
	if err := genesis.Validate(); err != nil {
		panic(err)
	}
	if err := k.SetParams(ctx, genesis.Params); err != nil {
		panic(err)
	}
	k.setPaused(ctx, genesis.Paused)
	k.setAttestorSet(ctx, genesis.Attestors)
	k.setNonce(ctx, genesis.Nonce)
	for _, request := range genesis.Requests {
		k.setRequest(ctx, request)
	}
	for _, result := range genesis.Results {
		k.setResult(ctx, result)
	}
}

func (k Keeper) ExportGenesis(ctx sdk.Context) types.GenesisState {
	return types.GenesisState{
		Params:    k.GetParams(ctx),
		Paused:    k.IsPaused(ctx),
		Attestors: k.GetAttestorSet(ctx),
		Nonce:     k.Nonce(ctx),
		Requests:  k.GetRequests(ctx),
		Results:   k.GetResults(ctx),
	}
}
