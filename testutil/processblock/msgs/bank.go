package msgs

import (
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	banktypes "github.com/sidiora-labs/paxeer-network/sdk/x/bank/types"
)

func Send(from sdk.AccAddress, to sdk.AccAddress, amount int64) *banktypes.MsgSend {
	return &banktypes.MsgSend{
		FromAddress: from.String(),
		ToAddress:   to.String(),
		Amount:      sdk.NewCoins(sdk.NewCoin("uhpx", sdk.NewInt(amount))),
	}
}
