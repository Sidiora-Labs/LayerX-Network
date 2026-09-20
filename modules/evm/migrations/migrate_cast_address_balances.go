package migrations

import (
	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

func MigrateCastAddressBalances(ctx sdk.Context, k *keeper.Keeper) (rerr error) {
	k.IteratePaxAddressMapping(ctx, func(evmAddr common.Address, paxAddr sdk.AccAddress) bool {
		castAddr := sdk.AccAddress(evmAddr[:])
		if balances := k.BankKeeper().SpendableCoins(ctx, castAddr); !balances.IsZero() {
			if err := k.BankKeeper().SendCoins(ctx, castAddr, paxAddr, balances); err != nil {
				logger.Error("error migrating balances from cast to real for address", "address", evmAddr, "err", err)
				rerr = err
				return true
			}
		}
		if wei := k.BankKeeper().GetWeiBalance(ctx, castAddr); !wei.IsZero() {
			if err := k.BankKeeper().SendCoinsAndWei(ctx, castAddr, paxAddr, sdk.ZeroInt(), wei); err != nil {
				logger.Error("error migrating wei from cast to real for address", "address", evmAddr, "err", err)
				rerr = err
				return true
			}
		}
		return false
	})
	return
}
