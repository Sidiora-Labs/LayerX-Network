package migrations

import (
	"github.com/paxeer-network/paxlog"
	"github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	bankkeeper "github.com/sidiora-labs/paxeer-network/sdk/x/bank/keeper"
)

var logger = paxlog.NewLogger("x", "evm", "migrations")

// This migration is to fix total supply mismatch caused by mishandled
// ante surplus
func FixTotalSupply(ctx sdk.Context, k *keeper.Keeper) error {
	balances := k.BankKeeper().GetAccountsBalances(ctx)
	correctSupply := sdk.ZeroInt()
	for _, balance := range balances {
		correctSupply = correctSupply.Add(balance.Coins.AmountOf(sdk.MustGetBaseDenom()))
	}
	totalWeiBalance := sdk.ZeroInt()
	k.BankKeeper().IterateAllWeiBalances(ctx, func(aa sdk.AccAddress, i sdk.Int) bool {
		totalWeiBalance = totalWeiBalance.Add(i)
		return false
	})
	weiInUhpx, weiRemainder := bankkeeper.SplitUhpxWeiAmount(totalWeiBalance)
	if !weiRemainder.IsZero() {
		logger.Error("wei total supply has been compromised as well; rounding up and adding to reserve")
		if err := k.BankKeeper().AddWei(ctx, k.AccountKeeper().GetModuleAddress(types.ModuleName), bankkeeper.OneUhpxInWei.Sub(weiRemainder)); err != nil {
			return err
		}
		weiInUhpx = weiInUhpx.Add(sdk.OneInt())
	}
	correctSupply = correctSupply.Add(weiInUhpx)
	currentSupply := k.BankKeeper().GetSupply(ctx, sdk.MustGetBaseDenom()).Amount
	if !currentSupply.Equal(correctSupply) {
		k.BankKeeper().SetSupply(ctx, sdk.NewCoin(sdk.MustGetBaseDenom(), correctSupply))
	}
	return nil
}
