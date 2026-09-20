package testutil

import (
	tmproto "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/types"
	dbm "github.com/tendermint/tm-db"

	"github.com/sidiora-labs/paxeer-network/sdk/store"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// DefaultContext creates a sdk.Context with a fresh MemDB that can be used in tests.
func DefaultContext(key sdk.StoreKey, tkey sdk.StoreKey) sdk.Context {
	db := dbm.NewMemDB()
	cms := store.NewCommitMultiStore(db)
	cms.MountStoreWithDB(key, sdk.StoreTypeIAVL, db)
	cms.MountStoreWithDB(tkey, sdk.StoreTypeTransient, db)
	err := cms.LoadLatestVersion()
	if err != nil {
		panic(err)
	}
	ctx := sdk.NewContext(cms, tmproto.Header{}, false)

	return ctx
}
