package types

import sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"

var (
	ErrUnauthorized     = sdkerrors.Register(ModuleName, 2, "unauthorized")
	ErrInvalidGenesis   = sdkerrors.Register(ModuleName, 3, "invalid genesis")
	ErrInvalidChain     = sdkerrors.Register(ModuleName, 4, "invalid chain registration")
	ErrUnknownChain     = sdkerrors.Register(ModuleName, 5, "chain not registered")
	ErrChainDisabled    = sdkerrors.Register(ModuleName, 6, "chain disabled")
	ErrInvalidAttestors = sdkerrors.Register(ModuleName, 7, "invalid attestor set")
	ErrInvalidCap       = sdkerrors.Register(ModuleName, 8, "invalid cap")
	ErrUnknownAsset     = sdkerrors.Register(ModuleName, 9, "asset not registered")
	ErrPaused           = sdkerrors.Register(ModuleName, 10, "bridge paused")
	ErrNullified        = sdkerrors.Register(ModuleName, 11, "remote event already bridged")
	ErrCapExceeded      = sdkerrors.Register(ModuleName, 12, "amount exceeds the asset cap")
	ErrBadSignature     = sdkerrors.Register(ModuleName, 13, "invalid attestor signature")
	ErrBelowThreshold   = sdkerrors.Register(ModuleName, 14, "attestor signatures below threshold")
	ErrVaultMismatch    = sdkerrors.Register(ModuleName, 15, "vault is not the registered vault")
	ErrInvalidRequest   = sdkerrors.Register(ModuleName, 16, "invalid bridge request")
	ErrInvalidParams    = sdkerrors.Register(ModuleName, 17, "invalid params")
)
