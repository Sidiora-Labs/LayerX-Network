package keeper

import (
	"fmt"

	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

func (k *Keeper) GetAccountFeeDenom(ctx sdk.Context, account common.Address) string {
	ctx = ctx.WithGasMeter(sdk.NewInfiniteGasMeterWithMultiplier(ctx))
	denom := ctx.KVStore(k.storeKey).Get(types.AccountFeeDenomKey(account))
	if denom == nil {
		return k.GetBaseDenom(ctx)
	}
	return string(denom)
}

func (k *Keeper) SetAccountFeeDenom(ctx sdk.Context, account common.Address, denom string) error {
	if !k.GetFeeTokenEnabled(ctx) {
		return fmt.Errorf("fee token preference: fee-token switch is off")
	}
	if allowed, _ := k.IsAllowedFeeDenom(ctx, denom); !allowed {
		return fmt.Errorf("fee token preference: denom %q is not allowed", denom)
	}
	ctx.KVStore(k.storeKey).Set(types.AccountFeeDenomKey(account), []byte(denom))
	return nil
}

func (k *Keeper) ClearAccountFeeDenom(ctx sdk.Context, account common.Address) {
	ctx.KVStore(k.storeKey).Delete(types.AccountFeeDenomKey(account))
}
