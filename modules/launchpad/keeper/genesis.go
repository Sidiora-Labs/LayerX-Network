package keeper

import (
	"fmt"

	"github.com/sidiora-labs/paxeer-network/modules/launchpad/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

func (k *Keeper) InitGenesis(ctx sdk.Context, gs types.GenesisState) {
	if err := gs.Validate(); err != nil {
		panic(fmt.Errorf("launchpad: invalid genesis: %w", err))
	}
	if err := k.setParams(ctx, gs.Params); err != nil {
		panic(err)
	}
	k.setProtocolFeesPending(ctx, gs.ProtocolFeesPending)
	for _, market := range gs.Markets {
		k.setMarket(ctx, market)
		k.indexMarket(ctx, market)
	}
	k.setUint64(ctx, types.MarketCountKey, uint64(len(gs.Markets)))
	for _, epoch := range gs.AirdropEpochs {
		k.setAirdropEpochAmount(ctx, epoch.Denom, epoch.Epoch, epoch.Amount)
	}
	for _, claim := range gs.AirdropClaims {
		k.setAirdropClaimed(ctx, claim.Denom, sdk.MustAccAddressFromBech32(claim.Holder), claim.Epoch)
	}
}

func (k *Keeper) ExportGenesis(ctx sdk.Context) *types.GenesisState {
	gs := types.DefaultGenesis()
	gs.Params = k.GetParams(ctx)
	gs.ProtocolFeesPending = k.GetProtocolFeesPending(ctx)
	k.IterateMarkets(ctx, func(market types.Market) bool {
		gs.Markets = append(gs.Markets, market)
		for epoch := uint64(1); epoch <= market.AirdropEpoch; epoch++ {
			if k.store(ctx).Has(types.AirdropEpochKey(market.Denom, epoch)) {
				gs.AirdropEpochs = append(gs.AirdropEpochs, types.AirdropEpochAmount{Denom: market.Denom, Epoch: epoch,
					Amount: k.GetAirdropEpochAmount(ctx, market.Denom, epoch)})
			}
		}
		return false
	})
	iterator := sdk.KVStorePrefixIterator(k.store(ctx), types.AirdropClaimPrefix)
	defer iterator.Close()
	for ; iterator.Valid(); iterator.Next() {
		denom, holder, epoch, ok := types.ParseAirdropClaimKey(iterator.Key())
		if !ok {
			panic(fmt.Errorf("launchpad: corrupt airdrop claim key %x", iterator.Key()))
		}
		gs.AirdropClaims = append(gs.AirdropClaims, types.AirdropClaim{Denom: denom,
			Holder: sdk.AccAddress(holder).String(), Epoch: epoch})
	}
	return gs
}
