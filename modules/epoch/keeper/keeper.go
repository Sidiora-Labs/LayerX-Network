package keeper

import (
	"github.com/sidiora-labs/paxeer-network/modules/epoch/types"
	"github.com/sidiora-labs/paxeer-network/sdk/codec"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	paramtypes "github.com/sidiora-labs/paxeer-network/sdk/x/params/types"
)

type (
	Keeper struct {
		cdc        codec.BinaryCodec
		storeKey   sdk.StoreKey
		memKey     sdk.StoreKey
		paramstore paramtypes.Subspace
		hooks      types.EpochHooks
	}
)

func NewKeeper(
	cdc codec.BinaryCodec,
	storeKey,
	memKey sdk.StoreKey,
	ps paramtypes.Subspace,
) *Keeper {
	// set KeyTable if it has not already been set
	if !ps.HasKeyTable() {
		ps = ps.WithKeyTable(types.ParamKeyTable())
	}

	return &Keeper{
		cdc:        cdc,
		storeKey:   storeKey,
		memKey:     memKey,
		paramstore: ps,
	}
}

func (k *Keeper) SetHooks(eh types.EpochHooks) *Keeper {
	if k.hooks != nil {
		panic("cannot set epochs hooks twice")
	}

	k.hooks = eh
	return k
}
