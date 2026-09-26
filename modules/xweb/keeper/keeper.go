// Package keeper holds the xweb state: the parameters, the attestor set, the
// pause, the request nonce, every request and every attested result. Fees are
// base-denom coins held by the module account between a request and its
// fulfilment or refund.
package keeper

import (
	"encoding/json"

	"github.com/sidiora-labs/paxeer-network/modules/xweb/types"
	"github.com/sidiora-labs/paxeer-network/sdk/store/prefix"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

type Keeper struct {
	storeKey   sdk.StoreKey
	bankKeeper types.BankKeeper
	evmKeeper  types.EVMKeeper
}

// NewKeeper builds the xweb keeper. The xweb module address must be able to
// receive coins: it escrows every pending request's fee.
func NewKeeper(storeKey sdk.StoreKey, bankKeeper types.BankKeeper, evmKeeper types.EVMKeeper) Keeper {
	return Keeper{storeKey: storeKey, bankKeeper: bankKeeper, evmKeeper: evmKeeper}
}

func (k Keeper) ModuleAddress() sdk.AccAddress { return types.ModuleAddress() }

func (k Keeper) set(ctx sdk.Context, key []byte, value interface{}) {
	encoded, err := json.Marshal(value)
	if err != nil {
		panic(err)
	}
	ctx.KVStore(k.storeKey).Set(key, encoded)
}

func (k Keeper) get(ctx sdk.Context, key []byte, value interface{}) bool {
	encoded := ctx.KVStore(k.storeKey).Get(key)
	if encoded == nil {
		return false
	}
	if err := json.Unmarshal(encoded, value); err != nil {
		panic(err)
	}
	return true
}

func (k Keeper) iterate(ctx sdk.Context, keyPrefix []byte, visit func(key, value []byte)) {
	iterator := prefix.NewStore(ctx.KVStore(k.storeKey), keyPrefix).Iterator(nil, nil)
	defer func() { _ = iterator.Close() }()
	for ; iterator.Valid(); iterator.Next() {
		visit(iterator.Key(), iterator.Value())
	}
}

func decode(value []byte, out interface{}) {
	if err := json.Unmarshal(value, out); err != nil {
		panic(err)
	}
}

func (k Keeper) GetParams(ctx sdk.Context) types.Params {
	var params types.Params
	if !k.get(ctx, types.ParamsKey, &params) {
		return types.DefaultParams(types.DefaultAuthority())
	}
	return params
}

func (k Keeper) SetParams(ctx sdk.Context, params types.Params) error {
	if err := params.Validate(); err != nil {
		return err
	}
	k.set(ctx, types.ParamsKey, params)
	return nil
}

// Fee is the amount of the base denom a request pays.
func (k Keeper) Fee(ctx sdk.Context) sdk.Int { return k.GetParams(ctx).Fee }

func (k Keeper) requireAuthority(ctx sdk.Context, authority string) error {
	if authority == "" || k.GetParams(ctx).Authority != authority {
		return types.ErrUnauthorized.Wrapf("%s is not the xweb authority", authority)
	}
	return nil
}

// Attestors.

func (k Keeper) GetAttestorSet(ctx sdk.Context) types.AttestorSet {
	var set types.AttestorSet
	k.get(ctx, types.AttestorSetKey, &set)
	return set
}

// Threshold is the number of distinct attestor signatures a fulfilment needs.
func (k Keeper) Threshold(ctx sdk.Context) uint32 { return k.GetAttestorSet(ctx).Threshold }

func (k Keeper) setAttestorSet(ctx sdk.Context, set types.AttestorSet) {
	k.set(ctx, types.AttestorSetKey, set)
}

// Pause.

func (k Keeper) IsPaused(ctx sdk.Context) bool {
	return ctx.KVStore(k.storeKey).Has(types.PausedKey)
}

func (k Keeper) setPaused(ctx sdk.Context, paused bool) {
	if paused {
		ctx.KVStore(k.storeKey).Set(types.PausedKey, []byte{1})
	} else {
		ctx.KVStore(k.storeKey).Delete(types.PausedKey)
	}
}

// Nonce is the id of the last request stored; the next request gets Nonce+1.
func (k Keeper) Nonce(ctx sdk.Context) uint64 {
	var nonce uint64
	k.get(ctx, types.NonceKey, &nonce)
	return nonce
}

func (k Keeper) setNonce(ctx sdk.Context, nonce uint64) { k.set(ctx, types.NonceKey, nonce) }

// Requests and results.

func (k Keeper) GetRequest(ctx sdk.Context, id uint64) (types.Request, bool) {
	var request types.Request
	found := k.get(ctx, types.RequestKey(id), &request)
	return request, found
}

func (k Keeper) GetRequests(ctx sdk.Context) []types.Request {
	var out []types.Request
	k.iterate(ctx, types.RequestPrefix, func(_, value []byte) {
		var request types.Request
		decode(value, &request)
		out = append(out, request)
	})
	return out
}

func (k Keeper) setRequest(ctx sdk.Context, request types.Request) {
	k.set(ctx, types.RequestKey(request.ID), request)
}

func (k Keeper) GetResult(ctx sdk.Context, id uint64) (types.Result, bool) {
	var result types.Result
	found := k.get(ctx, types.ResultKey(id), &result)
	return result, found
}

func (k Keeper) GetResults(ctx sdk.Context) []types.Result {
	var out []types.Result
	k.iterate(ctx, types.ResultPrefix, func(_, value []byte) {
		var result types.Result
		decode(value, &result)
		out = append(out, result)
	})
	return out
}

func (k Keeper) setResult(ctx sdk.Context, result types.Result) {
	k.set(ctx, types.ResultKey(result.RequestID), result)
}

func (k Keeper) feeCoins(amount sdk.Int) sdk.Coins {
	return sdk.NewCoins(sdk.NewCoin(sdk.MustGetBaseDenom(), amount))
}
