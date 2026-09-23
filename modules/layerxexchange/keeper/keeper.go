package keeper

import (
	"encoding/binary"

	"github.com/ethereum/go-ethereum/common"
	custodytypes "github.com/sidiora-labs/paxeer-network/modules/layerxcustody/types"
	"github.com/sidiora-labs/paxeer-network/modules/layerxexchange/types"
	"github.com/sidiora-labs/paxeer-network/sdk/codec"
	"github.com/sidiora-labs/paxeer-network/sdk/store/prefix"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
	authtypes "github.com/sidiora-labs/paxeer-network/sdk/x/auth/types"
	govtypes "github.com/sidiora-labs/paxeer-network/sdk/x/gov/types"
)

// Keeper records exchange intents for the LayerX intent router. It holds no
// funds: margin moves only into the layerxcustody module account through the
// custody keeper, and LayerX state is only ever read through a state proof
// against custody's AnchorReader. Matching happens on LayerX.
type Keeper struct {
	storeKey sdk.StoreKey
	cdc      codec.BinaryCodec

	custody   types.CustodyKeeper
	evmKeeper types.EVMKeeper
}

func NewKeeper(cdc codec.BinaryCodec, storeKey sdk.StoreKey, custody types.CustodyKeeper, evmKeeper types.EVMKeeper) *Keeper {
	return &Keeper{storeKey: storeKey, cdc: cdc, custody: custody, evmKeeper: evmKeeper}
}

func (k *Keeper) store(ctx sdk.Context) sdk.KVStore { return ctx.KVStore(k.storeKey) }

func (k *Keeper) GetParams(ctx sdk.Context) types.Params {
	var params types.Params
	bz := k.store(ctx).Get(types.ParamsKey)
	if bz == nil {
		return types.DefaultParams()
	}
	k.cdc.MustUnmarshal(bz, &params)
	return params
}

func (k *Keeper) SetParams(ctx sdk.Context, params types.Params) error {
	if err := params.Validate(); err != nil {
		return err
	}
	k.store(ctx).Set(types.ParamsKey, k.cdc.MustMarshal(&params))
	return nil
}

// SetMarket creates or replaces one market listing.
func (k *Keeper) SetMarket(ctx sdk.Context, market types.Market) error {
	if err := market.Validate(); err != nil {
		return err
	}
	params := k.GetParams(ctx)
	replaced := false
	for index := range params.Markets {
		if params.Markets[index].MarketId == market.MarketId {
			params.Markets[index], replaced = market, true
		}
	}
	if !replaced {
		params.Markets = append(params.Markets, market)
	}
	return k.SetParams(ctx, params)
}

// Authority is the bech32 account allowed to administer the exchange.
func (k *Keeper) Authority(ctx sdk.Context) string {
	if authority := k.GetParams(ctx).Authority; authority != "" {
		return authority
	}
	return authtypes.NewModuleAddress(govtypes.ModuleName).String()
}

func (k *Keeper) requireAuthority(ctx sdk.Context, signer string) error {
	if signer != k.Authority(ctx) {
		return sdkerrors.Wrapf(sdkerrors.ErrUnauthorized, "%s is not the exchange authority", signer)
	}
	return nil
}

// Custody returns the custody keeper margin moves through.
func (k *Keeper) Custody() types.CustodyKeeper { return k.custody }

func (k *Keeper) GetIntentCount(ctx sdk.Context) uint64 {
	bz := k.store(ctx).Get(types.IntentCountKey)
	if bz == nil {
		return 0
	}
	return binary.BigEndian.Uint64(bz)
}

func (k *Keeper) setIntentCount(ctx sdk.Context, count uint64) {
	k.store(ctx).Set(types.IntentCountKey, binary.BigEndian.AppendUint64(nil, count))
}

func (k *Keeper) GetOwnerNonce(ctx sdk.Context, owner common.Address) uint64 {
	bz := k.store(ctx).Get(types.OwnerNonceKey(owner))
	if bz == nil {
		return 0
	}
	return binary.BigEndian.Uint64(bz)
}

func (k *Keeper) setOwnerNonce(ctx sdk.Context, owner common.Address, nonce uint64) {
	k.store(ctx).Set(types.OwnerNonceKey(owner), binary.BigEndian.AppendUint64(nil, nonce))
}

func (k *Keeper) GetIntent(ctx sdk.Context, intentID [32]byte) (types.Intent, bool) {
	var intent types.Intent
	bz := k.store(ctx).Get(types.IntentKey(intentID))
	if bz == nil {
		return intent, false
	}
	k.cdc.MustUnmarshal(bz, &intent)
	return intent, true
}

func (k *Keeper) setIntent(ctx sdk.Context, intent types.Intent) {
	intentID, _ := custodytypes.ParseHash32(intent.IntentId)
	k.store(ctx).Set(types.IntentKey(intentID), k.cdc.MustMarshal(&intent))
}

func (k *Keeper) iterate(ctx sdk.Context, keyPrefix []byte, visit func(key, value []byte) bool) {
	iterator := prefix.NewStore(k.store(ctx), keyPrefix).Iterator(nil, nil)
	defer func() { _ = iterator.Close() }()
	for ; iterator.Valid(); iterator.Next() {
		if visit(iterator.Key(), iterator.Value()) {
			return
		}
	}
}

func (k *Keeper) IterateIntents(ctx sdk.Context, visit func(types.Intent) bool) {
	k.iterate(ctx, types.IntentPrefix, func(_, value []byte) bool {
		var intent types.Intent
		k.cdc.MustUnmarshal(value, &intent)
		return visit(intent)
	})
}
