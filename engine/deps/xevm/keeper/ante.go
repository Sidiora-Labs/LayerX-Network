package keeper

import (
	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/engine/deps/xevm/types"
	"github.com/sidiora-labs/paxeer-network/sdk/store/prefix"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

func (k *Keeper) AddAnteSurplus(ctx sdk.Context, txHash common.Hash, surplus sdk.Int) error {
	store := prefix.NewStore(ctx.TransientStore(k.transientStoreKey), types.AnteSurplusPrefix)
	bz, err := surplus.Marshal()
	if err != nil {
		return err
	}
	store.Set(txHash[:], bz)
	return nil
}

func (k *Keeper) GetAnteSurplusSum(ctx sdk.Context) sdk.Int {
	iter := prefix.NewStore(ctx.TransientStore(k.transientStoreKey), types.AnteSurplusPrefix).Iterator(nil, nil)
	defer func() { _ = iter.Close() }()
	res := sdk.ZeroInt()
	for ; iter.Valid(); iter.Next() {
		surplus := sdk.Int{}
		_ = surplus.Unmarshal(iter.Value())
		res = res.Add(surplus)
	}
	return res
}
