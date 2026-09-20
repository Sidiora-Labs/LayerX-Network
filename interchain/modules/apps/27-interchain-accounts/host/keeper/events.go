package keeper

import (
	"fmt"

	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"

	icatypes "github.com/sidiora-labs/paxeer-network/interchain/modules/apps/27-interchain-accounts/types"
	"github.com/sidiora-labs/paxeer-network/interchain/modules/core/exported"
)

// EmitAcknowledgementEvent emits an event signalling a successful or failed acknowledgement and including the error
// details if any.
func EmitAcknowledgementEvent(ctx sdk.Context, packet exported.PacketI, ack exported.Acknowledgement, err error) {
	var errorMsg string
	if err != nil {
		errorMsg = err.Error()
	}

	ctx.EventManager().EmitEvent(
		sdk.NewEvent(
			icatypes.EventTypePacket,
			sdk.NewAttribute(sdk.AttributeKeyModule, icatypes.ModuleName),
			sdk.NewAttribute(icatypes.AttributeKeyAckError, errorMsg),
			sdk.NewAttribute(icatypes.AttributeKeyHostChannelID, packet.GetDestChannel()),
			sdk.NewAttribute(icatypes.AttributeKeyAckSuccess, fmt.Sprintf("%t", ack.Success())),
		),
	)
}
