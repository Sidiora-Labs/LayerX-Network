package utils

import (
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"

	oracletypes "github.com/sidiora-labs/paxeer-network/modules/oracle/types"
)

func IsTxPrioritized(tx sdk.Tx) bool {
	for _, msg := range tx.GetMsgs() {
		switch msg.(type) {
		case *oracletypes.MsgAggregateExchangeRateVote:
			continue
		case *oracletypes.MsgDelegateFeedConsent:
			continue
		default:
			return false
		}
	}
	return true
}
