package types

import (
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
)

const StoreCodespace = "store"

var (
	ErrInvalidProof = sdkerrors.Register(StoreCodespace, 2, "invalid proof")
)
