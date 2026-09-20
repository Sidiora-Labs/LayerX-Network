package receipt

import (
	ethtypes "github.com/ethereum/go-ethereum/core/types"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	types2 "github.com/sidiora-labs/paxeer-network/storage/db_engine/types"
)

// RecoverReceiptStore exposes recoverReceiptStore for testing.
func RecoverReceiptStore(changelogPath string, db types2.StateStore) error {
	return recoverReceiptStore(changelogPath, db)
}

// GetLogsForTx exposes getLogsForTx for testing.
func GetLogsForTx(receipt *types.Receipt, logStartIndex uint) []*ethtypes.Log {
	return getLogsForTx(receipt, logStartIndex)
}
