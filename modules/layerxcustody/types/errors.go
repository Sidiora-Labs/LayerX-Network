package types

import (
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
)

var (
	ErrInvalidHex        = sdkerrors.Register(ModuleName, 2, "invalid hex identifier")
	ErrInvalidParams     = sdkerrors.Register(ModuleName, 3, "invalid params")
	ErrInvalidAsset      = sdkerrors.Register(ModuleName, 4, "invalid asset mapping")
	ErrUnknownAsset      = sdkerrors.Register(ModuleName, 5, "asset is not mapped")
	ErrAssetDisabled     = sdkerrors.Register(ModuleName, 6, "asset is disabled or paused")
	ErrInvalidDeposit    = sdkerrors.Register(ModuleName, 7, "invalid deposit")
	ErrInvalidClaim      = sdkerrors.Register(ModuleName, 8, "invalid claim")
	ErrNullifierUsed     = sdkerrors.Register(ModuleName, 9, "nullifier already used")
	ErrClaimNotReady     = sdkerrors.Register(ModuleName, 10, "claim is not ready")
	ErrClaimNotPending   = sdkerrors.Register(ModuleName, 11, "claim is not pending")
	ErrNotFinalized      = sdkerrors.Register(ModuleName, 12, "batch is not a finalized checkpoint")
	ErrNotAuthorized     = sdkerrors.Register(ModuleName, 13, "no sequencer authorization for batch")
	ErrExitNotEligible   = sdkerrors.Register(ModuleName, 14, "forced exit is not eligible")
	ErrExitConsumed      = sdkerrors.Register(ModuleName, 15, "forced exit already consumed")
	ErrInvalidExit       = sdkerrors.Register(ModuleName, 16, "invalid forced exit")
	ErrInvalidRelease    = sdkerrors.Register(ModuleName, 17, "invalid custody release")
	ErrInvalidCheckpoint = sdkerrors.Register(ModuleName, 18, "invalid checkpoint")
	ErrInvalidGenesis    = sdkerrors.Register(ModuleName, 19, "invalid genesis")
	ErrWrongNetwork      = sdkerrors.Register(ModuleName, 20, "evidence names another LayerX network")
	ErrInvalidProof      = sdkerrors.Register(ModuleName, 21, "LayerX evidence refused")
)
