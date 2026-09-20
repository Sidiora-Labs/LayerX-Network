//go:build gofuzz

package tests

import (
	"testing"

	"github.com/sidiora-labs/paxeer-network/consensus/abci/example/kvstore"
	"github.com/sidiora-labs/paxeer-network/consensus/config"
	"github.com/sidiora-labs/paxeer-network/consensus/internal/mempool"
)

func FuzzMempool(f *testing.F) {
	cfg := config.DefaultMempoolConfig()
	cfg.Broadcast = false

	mp := mempool.NewTxMempool(cfg.ToMempoolConfig(), kvstore.NewProxy(), mempool.NopMetrics(), mempool.NopTxConstraintsFetcher)

	f.Fuzz(func(t *testing.T, data []byte) {
		_, _ = mp.CheckTx(t.Context(), data)
	})
}
