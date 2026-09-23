package types

import (
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
)

var (
	ErrInvalidParams         = sdkerrors.Register(ModuleName, 2, "invalid launchpad params")
	ErrOverflow              = sdkerrors.Register(ModuleName, 3, "uint256 overflow")
	ErrDivisionByZero        = sdkerrors.Register(ModuleName, 4, "division by zero")
	ErrInsufficientInput     = sdkerrors.Register(ModuleName, 5, "insufficient input")
	ErrInsufficientLiquidity = sdkerrors.Register(ModuleName, 6, "insufficient liquidity")
	ErrVirtualFloorBreached  = sdkerrors.Register(ModuleName, 7, "virtual floor breached")
	ErrSlippageExceeded      = sdkerrors.Register(ModuleName, 8, "slippage exceeded")
	ErrDeadlineExpired       = sdkerrors.Register(ModuleName, 9, "deadline expired")
	ErrZeroAddress           = sdkerrors.Register(ModuleName, 10, "zero address")
	ErrZeroAmount            = sdkerrors.Register(ModuleName, 11, "zero amount")
	ErrUnknownMarket         = sdkerrors.Register(ModuleName, 12, "unknown market")
	ErrPaused                = sdkerrors.Register(ModuleName, 13, "market is paused")
	ErrNotPaused             = sdkerrors.Register(ModuleName, 14, "market is not paused")
	ErrNotGuardian           = sdkerrors.Register(ModuleName, 15, "caller is not the market guardian")
	ErrNotFeeRightsHolder    = sdkerrors.Register(ModuleName, 16, "caller does not hold the market's fee rights")
	ErrWrongStrategy         = sdkerrors.Register(ModuleName, 17, "wrong fee strategy")
	ErrInvalidStrategy       = sdkerrors.Register(ModuleName, 18, "invalid fee strategy")
	ErrNoFeesAccumulated     = sdkerrors.Register(ModuleName, 19, "no fees accumulated")
	ErrAirdropNotTriggered   = sdkerrors.Register(ModuleName, 20, "airdrop not triggered")
	ErrAlreadyClaimed        = sdkerrors.Register(ModuleName, 21, "airdrop already claimed")
	ErrInvalidMarket         = sdkerrors.Register(ModuleName, 22, "invalid market")
	ErrUnauthorized          = sdkerrors.Register(ModuleName, 23, "not the launchpad params authority")
	ErrInvalidGenesis        = sdkerrors.Register(ModuleName, 24, "invalid launchpad genesis")
)
