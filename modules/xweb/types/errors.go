package types

import sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"

var (
	ErrUnauthorized     = sdkerrors.Register(ModuleName, 2, "unauthorized")
	ErrInvalidGenesis   = sdkerrors.Register(ModuleName, 3, "invalid genesis")
	ErrInvalidParams    = sdkerrors.Register(ModuleName, 4, "invalid params")
	ErrInvalidAttestors = sdkerrors.Register(ModuleName, 5, "invalid attestor set")
	ErrInvalidThreshold = sdkerrors.Register(ModuleName, 6, "threshold is not a majority of the attestor set")
	ErrPaused           = sdkerrors.Register(ModuleName, 7, "xweb paused")
	ErrUnknownKind      = sdkerrors.Register(ModuleName, 8, "unknown request kind")
	ErrPayloadSize      = sdkerrors.Register(ModuleName, 9, "payload empty or over the payload cap")
	ErrCallbackGas      = sdkerrors.Register(ModuleName, 10, "callback gas zero or over the callback cap")
	ErrWrongFee         = sdkerrors.Register(ModuleName, 11, "paid amount is not the current fee")
	ErrUnknownRequest   = sdkerrors.Register(ModuleName, 12, "request not found")
	ErrAlreadyFulfilled = sdkerrors.Register(ModuleName, 13, "request already fulfilled")
	ErrRefunded         = sdkerrors.Register(ModuleName, 14, "request already refunded")
	ErrResponseTooLarge = sdkerrors.Register(ModuleName, 15, "response over the stored response bound")
	ErrInvalidLength    = sdkerrors.Register(ModuleName, 16, "full length shorter than the response")
	ErrBadSignature     = sdkerrors.Register(ModuleName, 17, "invalid attestor signature")
	ErrBelowThreshold   = sdkerrors.Register(ModuleName, 18, "attestor signatures below threshold")
	ErrNotExpired       = sdkerrors.Register(ModuleName, 19, "request not yet past its timeout")
	ErrUnknownAttestor  = sdkerrors.Register(ModuleName, 20, "attestor not registered")
	ErrCallbackRecorded = sdkerrors.Register(ModuleName, 21, "callback outcome already recorded")
	ErrInvalidRequest   = sdkerrors.Register(ModuleName, 22, "invalid request")
)
