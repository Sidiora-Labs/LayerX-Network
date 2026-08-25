package pebbledb

import (
	"fmt"

	"github.com/sidiora-labs/paxeer-network/storage/db_engine/dbcache"
	"github.com/sidiora-labs/paxeer-network/storage/db_engine/types"
)

// TableIters returns the number of open SSTable iterators for db.
// db may be wrapped in one or more cachedKeyValueDB layers.
func TableIters(db types.KeyValueDB) (int64, error) {
	inner := dbcache.Unwrap(db)
	p, ok := inner.(*pebbleDB)
	if !ok {
		return 0, fmt.Errorf("expected pebbleDB, got %T", inner)
	}
	return p.db.Metrics().TableIters, nil
}
