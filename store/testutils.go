package store

import (
	"github.com/sidiora-labs/paxeer-network/sdk/store/cachekv"
	"github.com/sidiora-labs/paxeer-network/sdk/store/cachemulti"
	"github.com/sidiora-labs/paxeer-network/sdk/store/dbadapter"
	storetypes "github.com/sidiora-labs/paxeer-network/sdk/store/types"
	"github.com/sidiora-labs/paxeer-network/sdk/types"
	dbm "github.com/tendermint/tm-db"
)

func NewTestKVStore() types.KVStore {
	mem := dbadapter.Store{DB: dbm.NewMemDB()}
	return cachekv.NewStore(mem, storetypes.NewKVStoreKey("test"), storetypes.DefaultCacheSizeLimit)
}

func NewTestCacheMultiStore(stores map[types.StoreKey]types.CacheWrapper) types.CacheMultiStore {
	return cachemulti.NewStore(dbm.NewMemDB(), stores, map[string]types.StoreKey{}, nil, nil, nil, 0)
}
