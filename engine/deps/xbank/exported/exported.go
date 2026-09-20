package exported

import (
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// GenesisBalance defines a genesis balance interface that allows for account
// address and balance retrieval.
type GenesisBalance interface {
	GetAddress() sdk.AccAddress
	GetCoins() sdk.Coins
}
