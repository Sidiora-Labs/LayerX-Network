package state

import (
	tmstate "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/state"
	"github.com/sidiora-labs/paxeer-network/consensus/types"
)

func ABCIResponsesResultsHash(ar *tmstate.ABCIResponses) []byte {
	return types.NewResults(ar.DeliverTxs).Hash()
}
