package types

// DONTCOVER

import (
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
)

// modules/epoch module sentinel errors
var (
	ErrParsingPaxEpochQuery = sdkerrors.Register(ModuleName, 2, "Error parsing PaxEpochQuery")
	ErrGettingEpoch         = sdkerrors.Register(ModuleName, 3, "Error while getting epoch")
	ErrEncodingEpoch        = sdkerrors.Register(ModuleName, 4, "Error encoding epoch as JSON")
	ErrUnknownPaxEpochQuery = sdkerrors.Register(ModuleName, 6, "Error unknown pax epoch query")
)
