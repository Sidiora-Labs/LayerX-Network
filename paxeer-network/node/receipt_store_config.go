package app

import (
	"github.com/spf13/cast"

	"github.com/sidiora-labs/paxeer-network/sdk/server"
	"github.com/sidiora-labs/paxeer-network/storage/common/utils"
	paxdbconfig "github.com/sidiora-labs/paxeer-network/storage/config"
)

const (
	receiptStoreBackendKey              = "receipt-store.rs-backend"
	receiptStoreDBDirectoryKey          = "receipt-store.db-directory"
	receiptStoreAsyncWriteBufferKey     = "receipt-store.async-write-buffer"
	receiptStorePruneIntervalSecondsKey = "receipt-store.prune-interval-seconds"
)

func readReceiptStoreConfig(homePath string, appOpts paxdbconfig.AppOptions) (paxdbconfig.ReceiptStoreConfig, error) {
	receiptConfig, err := paxdbconfig.ReadReceiptConfig(appOpts)
	if err != nil {
		return receiptConfig, err
	}
	if receiptConfig.DBDirectory == "" {
		receiptConfig.DBDirectory = utils.GetReceiptStorePath(homePath, receiptConfig.Backend)
	}
	receiptConfig.KeepRecent = cast.ToInt(appOpts.Get(server.FlagMinRetainBlocks))
	return receiptConfig, nil
}
