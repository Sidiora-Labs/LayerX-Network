// Package launchpadtest runs the launchpad keeper on the shared test
// application's state before the app mounts the launchpad store.
package launchpadtest

import (
	"github.com/sidiora-labs/paxeer-network/modules/launchpad/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/launchpad/types"
	tokenfactorykeeper "github.com/sidiora-labs/paxeer-network/modules/tokenfactory/keeper"
	app "github.com/sidiora-labs/paxeer-network/node"
	"github.com/sidiora-labs/paxeer-network/sdk/store/cachekv"
	"github.com/sidiora-labs/paxeer-network/sdk/store/dbadapter"
	storetypes "github.com/sidiora-labs/paxeer-network/sdk/store/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	dbm "github.com/tendermint/tm-db"
)

// overlay is a cache multistore that serves the launchpad store key from its
// own branch and every other key from the parent, and branches and writes
// both together the way cachemulti does.
type parent = storetypes.CacheMultiStore

type overlay struct {
	parent
	key   storetypes.StoreKey
	store storetypes.CacheKVStore
}

func (o *overlay) GetKVStore(key storetypes.StoreKey) storetypes.KVStore {
	if key.Name() == o.key.Name() {
		return o.store
	}
	return o.parent.GetKVStore(key)
}

func (o *overlay) GetStore(key storetypes.StoreKey) storetypes.Store {
	if key.Name() == o.key.Name() {
		return o.store
	}
	return o.parent.GetStore(key)
}

func (o *overlay) CacheMultiStore() storetypes.CacheMultiStore {
	return &overlay{parent: o.parent.CacheMultiStore(), key: o.key,
		store: cachekv.NewStore(o.store, o.key, storetypes.DefaultCacheSizeLimit)}
}

func (o *overlay) Write() {
	o.store.Write()
	o.parent.Write()
}

// WithStore returns ctx branched so that key resolves to a fresh in-memory
// store layered like the app's mounted stores.
func WithStore(ctx sdk.Context, key storetypes.StoreKey) sdk.Context {
	base := cachekv.NewStore(dbadapter.Store{DB: dbm.NewMemDB()}, key, storetypes.DefaultCacheSizeLimit)
	return ctx.WithMultiStore(&overlay{parent: ctx.MultiStore().CacheMultiStore(), key: key, store: base})
}

// NewKeeper builds the launchpad keeper over the test app's real bank,
// tokenfactory and EVM keepers and returns ctx carrying its store.
func NewKeeper(testApp *app.App, ctx sdk.Context) (*keeper.Keeper, sdk.Context) {
	key := sdk.NewKVStoreKey(types.StoreKey)
	k := keeper.NewKeeper(key, testApp.BankKeeper, tokenfactorykeeper.NewMsgServerImpl(testApp.TokenFactoryKeeper), &testApp.EvmKeeper)
	return k, WithStore(ctx, key)
}
