// Package keeper holds the LayerX bridge state: the remote chain registry,
// the attestor set, per-asset caps, the global pause, the nullifier set of
// consumed remote events and the outbound nonces. Bridged assets are
// tokenfactory denoms whose admin is the bridge module account; every mint and
// burn goes through the tokenfactory keeper.
package keeper

import (
	"encoding/json"
	"fmt"

	"github.com/sidiora-labs/paxeer-network/modules/layerxbridge/types"
	tokenfactorykeeper "github.com/sidiora-labs/paxeer-network/modules/tokenfactory/keeper"
	tokenfactorytypes "github.com/sidiora-labs/paxeer-network/modules/tokenfactory/types"
	"github.com/sidiora-labs/paxeer-network/sdk/store/prefix"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

type Keeper struct {
	storeKey        sdk.StoreKey
	bankKeeper      types.BankKeeper
	evmKeeper       types.EVMKeeper
	tokenFactory    tokenfactorykeeper.Keeper
	tokenFactoryMsg tokenfactorytypes.MsgServer
}

// NewKeeper builds the bridge keeper. The bridge module address must be able
// to receive coins from the tokenfactory module account: it is the admin the
// tokenfactory mints to before the bridge forwards the coins.
func NewKeeper(storeKey sdk.StoreKey, bankKeeper types.BankKeeper, evmKeeper types.EVMKeeper,
	tokenFactory tokenfactorykeeper.Keeper) Keeper {
	return Keeper{
		storeKey:        storeKey,
		bankKeeper:      bankKeeper,
		evmKeeper:       evmKeeper,
		tokenFactory:    tokenFactory,
		tokenFactoryMsg: tokenfactorykeeper.NewMsgServerImpl(tokenFactory),
	}
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

func (k Keeper) requireAuthority(ctx sdk.Context, authority string) error {
	if authority == "" || k.GetParams(ctx).Authority != authority {
		return types.ErrUnauthorized.Wrapf("%s is not the bridge authority", authority)
	}
	return nil
}

// Chains.

func (k Keeper) GetChain(ctx sdk.Context, chainID uint64) (types.Chain, bool) {
	var chain types.Chain
	found := k.get(ctx, types.ChainKey(chainID), &chain)
	return chain, found
}

func (k Keeper) GetChains(ctx sdk.Context) []types.Chain {
	var out []types.Chain
	k.iterate(ctx, types.ChainPrefix, func(_, value []byte) {
		var chain types.Chain
		decode(value, &chain)
		out = append(out, chain)
	})
	return out
}

func (k Keeper) setChain(ctx sdk.Context, chain types.Chain) {
	k.set(ctx, types.ChainKey(chain.ChainID), chain)
}

// Attestors.

func (k Keeper) GetAttestorSet(ctx sdk.Context) types.AttestorSet {
	var set types.AttestorSet
	k.get(ctx, types.AttestorSetKey, &set)
	return set
}

func (k Keeper) setAttestorSet(ctx sdk.Context, set types.AttestorSet) {
	k.set(ctx, types.AttestorSetKey, set)
}

// Assets and caps.

func (k Keeper) GetAsset(ctx sdk.Context, chainID uint64, asset types.Address20) (types.BridgedAsset, bool) {
	var record types.BridgedAsset
	found := k.get(ctx, types.AssetKey(chainID, asset), &record)
	return record, found
}

func (k Keeper) GetAssetByDenom(ctx sdk.Context, denom string) (types.BridgedAsset, bool) {
	var record types.BridgedAsset
	found := k.get(ctx, types.DenomKey(denom), &record)
	return record, found
}

func (k Keeper) GetAssets(ctx sdk.Context) []types.BridgedAsset {
	var out []types.BridgedAsset
	k.iterate(ctx, types.AssetPrefix, func(_, value []byte) {
		var record types.BridgedAsset
		decode(value, &record)
		out = append(out, record)
	})
	return out
}

func (k Keeper) setAsset(ctx sdk.Context, record types.BridgedAsset) {
	k.set(ctx, types.AssetKey(record.ChainID, record.Asset), record)
	k.set(ctx, types.DenomKey(record.Denom), record)
}

func (k Keeper) GetCap(ctx sdk.Context, denom string) (types.Cap, bool) {
	var c types.Cap
	found := k.get(ctx, types.CapKey(denom), &c)
	return c, found
}

func (k Keeper) GetCaps(ctx sdk.Context) []types.Cap {
	var out []types.Cap
	k.iterate(ctx, types.CapPrefix, func(_, value []byte) {
		var c types.Cap
		decode(value, &c)
		out = append(out, c)
	})
	return out
}

func (k Keeper) setCap(ctx sdk.Context, c types.Cap) { k.set(ctx, types.CapKey(c.Denom), c) }

// InFlight is the bridged supply of denom outstanding on Paxeer.
func (k Keeper) InFlight(ctx sdk.Context, denom string) sdk.Int {
	var amount sdk.Int
	if !k.get(ctx, types.InFlightKey(denom), &amount) {
		return sdk.ZeroInt()
	}
	return amount
}

func (k Keeper) setInFlight(ctx sdk.Context, denom string, amount sdk.Int) {
	k.set(ctx, types.InFlightKey(denom), amount)
}

func (k Keeper) GetInFlight(ctx sdk.Context) []types.InFlight {
	var out []types.InFlight
	k.iterate(ctx, types.InFlightPrefix, func(key, value []byte) {
		var amount sdk.Int
		decode(value, &amount)
		out = append(out, types.InFlight{Denom: string(key), Amount: amount})
	})
	return out
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

// Nullifiers.

func (k Keeper) IsNullified(ctx sdk.Context, nullifier types.Nullifier) bool {
	return ctx.KVStore(k.storeKey).Has(types.NullifierKey(nullifier))
}

func (k Keeper) setNullifier(ctx sdk.Context, nullifier types.Nullifier) {
	k.set(ctx, types.NullifierKey(nullifier), nullifier)
}

func (k Keeper) GetNullifiers(ctx sdk.Context) []types.Nullifier {
	var out []types.Nullifier
	k.iterate(ctx, types.NullifierPrefix, func(_, value []byte) {
		var nullifier types.Nullifier
		decode(value, &nullifier)
		out = append(out, nullifier)
	})
	return out
}

// Outbound nonces.

func (k Keeper) OutboundNonce(ctx sdk.Context, chainID uint64) uint64 {
	var nonce uint64
	k.get(ctx, types.OutboundNonceChainKey(chainID), &nonce)
	return nonce
}

func (k Keeper) setOutboundNonce(ctx sdk.Context, chainID, nonce uint64) {
	k.set(ctx, types.OutboundNonceChainKey(chainID), nonce)
}

func (k Keeper) GetOutboundNonces(ctx sdk.Context) []types.OutboundNonce {
	var out []types.OutboundNonce
	for _, chain := range k.GetChains(ctx) {
		if nonce := k.OutboundNonce(ctx, chain.ChainID); nonce != 0 {
			out = append(out, types.OutboundNonce{ChainID: chain.ChainID, Nonce: nonce})
		}
	}
	return out
}

func hexBytes(value []byte) string { return fmt.Sprintf("%x", value) }
