package receipt

import (
	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	storetypes "github.com/sidiora-labs/paxeer-network/sdk/store/types"
	"github.com/sidiora-labs/paxeer-network/sdk/testutil"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

func newTestContext() (sdk.Context, storetypes.StoreKey) {
	storeKey := storetypes.NewKVStoreKey("evm")
	tkey := storetypes.NewTransientStoreKey("evm_transient")
	ctx := testutil.DefaultContext(storeKey, tkey).WithBlockHeight(1)
	return ctx, storeKey
}

func makeTestReceipt(txHash common.Hash, blockNumber uint64, txIndex uint32, addr common.Address, topics []common.Hash) *types.Receipt {
	topicHex := make([]string, 0, len(topics))
	for _, topic := range topics {
		topicHex = append(topicHex, topic.Hex())
	}

	return &types.Receipt{
		TxHashHex:        txHash.Hex(),
		BlockNumber:      blockNumber,
		TransactionIndex: txIndex,
		Logs: []*types.Log{
			{
				Address: addr.Hex(),
				Topics:  topicHex,
				Data:    []byte{0x1},
				Index:   0,
			},
		},
	}
}
