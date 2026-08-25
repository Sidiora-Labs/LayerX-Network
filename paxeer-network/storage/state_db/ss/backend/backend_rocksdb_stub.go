//go:build !rocksdbBackend

package backend

import (
	"fmt"

	"github.com/sidiora-labs/paxeer-network/storage/config"
	"github.com/sidiora-labs/paxeer-network/storage/db_engine/types"
)

func openRocksDB(_ string, _ config.StateStoreConfig) (types.StateStore, error) {
	return nil, fmt.Errorf("rocksdb backend not available: rebuild with -tags=rocksdbBackend")
}
