package migrations

import (
	"github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

func MigrateBlockBloom(ctx sdk.Context, k *keeper.Keeper) error {
	k.SetLegacyBlockBloomCutoffHeight(ctx)

	prefsToDelete := [][]byte{}
	k.IterateAll(ctx, types.BlockBloomPrefix, func(key, _ []byte) bool {
		if len(key) > 0 {
			prefsToDelete = append(prefsToDelete, key)
		}
		return false
	})
	store := k.PrefixStore(ctx, types.BlockBloomPrefix)
	for _, pref := range prefsToDelete {
		store.Delete(pref)
	}

	return nil
}
