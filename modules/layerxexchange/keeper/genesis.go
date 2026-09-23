package keeper

import (
	"encoding/binary"

	"github.com/ethereum/go-ethereum/common"
	custodytypes "github.com/sidiora-labs/paxeer-network/modules/layerxcustody/types"
	"github.com/sidiora-labs/paxeer-network/modules/layerxexchange/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// InitGenesis loads a validated genesis state.
func (k *Keeper) InitGenesis(ctx sdk.Context, gs types.GenesisState) {
	if err := gs.Validate(); err != nil {
		panic(err)
	}
	if err := k.SetParams(ctx, gs.Params); err != nil {
		panic(err)
	}
	k.setIntentCount(ctx, gs.IntentCount)
	for _, intent := range gs.Intents {
		k.setIntent(ctx, intent)
	}
	for _, nonce := range gs.OwnerNonces {
		owner, _ := custodytypes.ParseAddress(nonce.Owner)
		k.setOwnerNonce(ctx, owner, nonce.Nonce)
	}
}

// ExportGenesis exports the full exchange state in store order.
func (k *Keeper) ExportGenesis(ctx sdk.Context) *types.GenesisState {
	gs := &types.GenesisState{Params: k.GetParams(ctx), IntentCount: k.GetIntentCount(ctx)}
	k.IterateIntents(ctx, func(intent types.Intent) bool { gs.Intents = append(gs.Intents, intent); return false })
	k.iterate(ctx, types.OwnerNoncePrefix, func(key, value []byte) bool {
		gs.OwnerNonces = append(gs.OwnerNonces, types.OwnerNonce{Owner: custodytypes.Address(common.BytesToAddress(key)),
			Nonce: binary.BigEndian.Uint64(value)})
		return false
	})
	return gs
}
