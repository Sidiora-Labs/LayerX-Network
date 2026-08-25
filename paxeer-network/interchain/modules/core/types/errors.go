package types

import (
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
)

var (
	// ErrInboundDisabled / ErrOutboundDisabled
	ErrInboundDisabled  = sdkerrors.Register("ibc", 101, "ibc inbound disabled")
	ErrOutboundDisabled = sdkerrors.Register("ibc", 102, "ibc outbound disabled")
)
