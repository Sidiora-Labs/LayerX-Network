// Package testutil gives tests a layerxbridge keeper over the real
// application keepers before the application mounts the bridge store. The
// bridge store is a real in-memory KV store layered into the context's multi
// store; every other store is the application's.
package testutil

import (
	"github.com/sidiora-labs/paxeer-network/modules/layerxbridge/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/layerxbridge/types"
	app "github.com/sidiora-labs/paxeer-network/node"
	"github.com/sidiora-labs/paxeer-network/sdk/store/cachekv"
	"github.com/sidiora-labs/paxeer-network/sdk/store/dbadapter"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	dbm "github.com/tendermint/tm-db"
)

const cacheSize = 1024

// base names the embedded application store apart from the CacheMultiStore
// method overlay overrides.
type base = sdk.CacheMultiStore

// overlay is a cache multi store that serves one extra KV store and branches
// it together with the rest.
type overlay struct {
	base
	key   sdk.StoreKey
	store sdk.CacheKVStore
}

func (o overlay) GetKVStore(key sdk.StoreKey) sdk.KVStore {
	if key == o.key {
		return o.store
	}
	return o.base.GetKVStore(key)
}

func (o overlay) GetStore(key sdk.StoreKey) sdk.Store {
	if key == o.key {
		return o.store
	}
	return o.base.GetStore(key)
}

func (o overlay) CacheMultiStore() sdk.CacheMultiStore {
	return overlay{base: o.base.CacheMultiStore(), key: o.key,
		store: cachekv.NewStore(o.store, o.key, cacheSize)}
}

func (o overlay) Write() {
	o.base.Write()
	o.store.Write()
}

// WithBridgeStore branches ctx and serves key from a fresh in-memory store.
func WithBridgeStore(ctx sdk.Context, key sdk.StoreKey) sdk.Context {
	base := dbadapter.Store{DB: dbm.NewMemDB()}
	return ctx.WithMultiStore(overlay{base: ctx.MultiStore().CacheMultiStore(), key: key,
		store: cachekv.NewStore(base, key, cacheSize)})
}

// NewKeeper builds the bridge keeper on the application's bank, EVM and
// tokenfactory keepers and returns ctx branched with the bridge store.
func NewKeeper(testApp *app.App, ctx sdk.Context) (keeper.Keeper, sdk.Context) {
	key := sdk.NewKVStoreKey(types.StoreKey)
	k := keeper.NewKeeper(key, testApp.BankKeeper, &testApp.EvmKeeper, testApp.TokenFactoryKeeper)
	return k, WithBridgeStore(ctx, key)
}
