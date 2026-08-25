package backend

import (
	"github.com/sidiora-labs/paxeer-network/storage/config"
	"github.com/sidiora-labs/paxeer-network/storage/db_engine/pebbledb/mvcc"
	"github.com/sidiora-labs/paxeer-network/storage/db_engine/types"
)

func openPebbleDB(dbHome string, cfg config.StateStoreConfig) (types.StateStore, error) {
	return mvcc.OpenDB(dbHome, cfg)
}
