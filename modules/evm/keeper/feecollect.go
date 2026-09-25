package keeper

import (
	"fmt"

	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	authtypes "github.com/sidiora-labs/paxeer-network/sdk/x/auth/types"
)

func (k *Keeper) GetFeeTokenDistribution(ctx sdk.Context) bool {
	distribute := types.DefaultFeeTokenDistribution
	k.Paramstore.GetIfExists(ctx, types.KeyFeeTokenDistribution, &distribute)
	return distribute
}

func (k *Keeper) RouteCollectedFeeTokens(ctx sdk.Context) error {
	if k.GetFeeTokenDistribution(ctx) {
		return nil
	}
	denoms := k.GetAllowedFeeDenoms(ctx)
	if len(denoms) == 0 {
		return nil
	}
	collector := k.accountKeeper.GetModuleAddress(authtypes.FeeCollectorName)
	coins := sdk.NewCoins()
	for _, entry := range denoms {
		if entry.Denom == k.GetBaseDenom(ctx) {
			continue
		}
		coin := k.bankKeeper.GetBalance(ctx, collector, entry.Denom)
		if coin.IsPositive() {
			coins = coins.Add(coin)
		}
	}
	if coins.Empty() {
		return nil
	}
	if k.accountKeeper.GetModuleAddress(types.FeeTokenHoldingAccount) == nil {
		return fmt.Errorf("fee token holding module account is not registered")
	}
	return k.bankKeeper.SendCoinsFromModuleToModule(ctx, authtypes.FeeCollectorName, types.FeeTokenHoldingAccount, coins)
}
