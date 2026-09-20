package keeper

import (
	"fmt"

	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
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

func (k *Keeper) GetAnteSurplusSum(ctx sdk.Context) (sdk.Int, error) {
	iter := prefix.NewStore(ctx.TransientStore(k.transientStoreKey), types.AnteSurplusPrefix).Iterator(nil, nil)
	defer func() { _ = iter.Close() }()
	res := sdk.ZeroInt()
	for ; iter.Valid(); iter.Next() {
		surplus := sdk.Int{}
		if err := surplus.Unmarshal(iter.Value()); err != nil {
			return sdk.ZeroInt(), fmt.Errorf("decode ante surplus for transaction %x: %w", iter.Key(), err)
		}
		res = res.Add(surplus)
	}
	return res, nil
}
