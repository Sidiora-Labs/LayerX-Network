package migrations

import (
	"github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// MigrateFeeTokenParams adds the fee-token parameters to a store that predates
// them. Each pair is written only when the store does not carry it yet, so a
// parameter governance has already set keeps its stored value and a store that
// already carries all five is left untouched.
func MigrateFeeTokenParams(ctx sdk.Context, k *keeper.Keeper) error {
	defaults := types.DefaultParams()
	for _, pair := range []struct {
		key   []byte
		value interface{}
	}{
		{types.KeyFeeTokenEnabled, defaults.FeeTokenEnabled},
		{types.KeyAllowedFeeDenoms, defaults.AllowedFeeDenoms},
		{types.KeyMaxFeeTokenSpread, defaults.MaxFeeTokenSpread},
		{types.KeyMaxFeeTokenRateAge, defaults.MaxFeeTokenRateAge},
		{types.KeyFeeTokenDistribution, defaults.FeeTokenDistribution},
	} {
		if k.Paramstore.Has(ctx, pair.key) {
			continue
		}
		if err := k.Paramstore.Validate(ctx, pair.key, pair.value); err != nil {
			return err
		}
		k.Paramstore.Set(ctx, pair.key, pair.value)
	}
	return nil
}
