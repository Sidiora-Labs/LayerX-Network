//go:build rocksdbBackend

package backend

import (
	"os"

	"github.com/sidiora-labs/paxeer-network/storage/config"
	"github.com/sidiora-labs/paxeer-network/storage/db_engine/rocksdb/mvcc"
	"github.com/sidiora-labs/paxeer-network/storage/db_engine/types"
)

func openRocksDB(dbHome string, cfg config.StateStoreConfig) (types.StateStore, error) {
	// RocksDB's CreateIfMissing only creates the leaf directory
	if err := os.MkdirAll(dbHome, 0o750); err != nil {
		return nil, err
	}
	return mvcc.OpenDB(dbHome, cfg)
}
