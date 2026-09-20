package backend

import (
	"github.com/sidiora-labs/paxeer-network/storage/config"
	"github.com/sidiora-labs/paxeer-network/storage/db_engine/types"
)

// OpenFunc creates a StateStore from a data directory and config.
type OpenFunc func(dbHome string, cfg config.StateStoreConfig) (types.StateStore, error)

// ResolveBackend returns the OpenFunc for the given backend name.
// Defaults to PebbleDB. RocksDB is available only when built with -tags=rocksdbBackend.
func ResolveBackend(backendName string) OpenFunc {
	switch backendName {
	case config.RocksDBBackend:
		return openRocksDB
	default:
		return openPebbleDB
	}
}
