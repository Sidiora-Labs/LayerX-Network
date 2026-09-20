package ss

import (
	"github.com/sidiora-labs/paxeer-network/storage/config"
	"github.com/sidiora-labs/paxeer-network/storage/db_engine/types"
	"github.com/sidiora-labs/paxeer-network/storage/state_db/ss/composite"
)

// NewStateStore creates a CompositeStateStore which handles both Cosmos and EVM data.
// The backend (pebbledb or rocksdb) is resolved at compile time via build-tag-gated
// files in the backend package. When WriteMode/ReadMode are both cosmos_only (the default),
// the EVM stores are not opened and the composite store behaves identically to a plain cosmos state store.
func NewStateStore(homeDir string, ssConfig config.StateStoreConfig) (types.StateStore, error) {
	return composite.NewCompositeStateStore(ssConfig, homeDir)
}
