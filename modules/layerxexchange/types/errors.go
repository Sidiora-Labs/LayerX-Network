package types

import (
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
)

var (
	ErrInvalidParams   = sdkerrors.Register(ModuleName, 2, "invalid params")
	ErrInvalidMarket   = sdkerrors.Register(ModuleName, 3, "invalid market")
	ErrUnknownMarket   = sdkerrors.Register(ModuleName, 4, "market is not listed or is disabled")
	ErrNotMarginAsset  = sdkerrors.Register(ModuleName, 5, "asset is not the margin asset of an enabled market")
	ErrInvalidIntent   = sdkerrors.Register(ModuleName, 6, "invalid exchange intent")
	ErrIntentExists    = sdkerrors.Register(ModuleName, 7, "intent already recorded")
	ErrNotFinalized    = sdkerrors.Register(ModuleName, 8, "batch is not a finalized checkpoint")
	ErrInvalidProof    = sdkerrors.Register(ModuleName, 9, "LayerX state proof refused")
	ErrStateMismatch   = sdkerrors.Register(ModuleName, 10, "state proof does not prove the requested entry")
	ErrInvalidGenesis  = sdkerrors.Register(ModuleName, 11, "invalid genesis")
	ErrKeeperMissing   = sdkerrors.Register(ModuleName, 12, "layerxexchange keeper is not wired")
	ErrEvidenceTooLong = sdkerrors.Register(ModuleName, 13, "state proof exceeds the exchange bound")
)
