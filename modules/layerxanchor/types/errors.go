package types

import sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"

var (
	ErrUnauthorized          = sdkerrors.Register(ModuleName, 2, "unauthorized")
	ErrInvalidGenesis        = sdkerrors.Register(ModuleName, 3, "invalid genesis")
	ErrSequencerUnauthorized = sdkerrors.Register(ModuleName, 4, "no sequencer authorization covers the batch")
	ErrCertificate           = sdkerrors.Register(ModuleName, 5, "invalid checkpoint certificate")
	ErrContinuity            = sdkerrors.Register(ModuleName, 6, "checkpoint does not continue the finalized chain")
	ErrCheckpointFinal       = sdkerrors.Register(ModuleName, 7, "checkpoint already finalized")
	ErrCheckpointUnknown     = sdkerrors.Register(ModuleName, 8, "checkpoint unknown")
	ErrGuarantorExists       = sdkerrors.Register(ModuleName, 9, "guarantor already registered")
	ErrGuarantorUnknown      = sdkerrors.Register(ModuleName, 10, "guarantor unknown")
	ErrBond                  = sdkerrors.Register(ModuleName, 11, "invalid bond action")
	ErrUnbondingImmature     = sdkerrors.Register(ModuleName, 12, "unbonding delay has not elapsed")
	ErrEvidence              = sdkerrors.Register(ModuleName, 13, "invalid equivocation evidence")
	ErrAlreadySlashed        = sdkerrors.Register(ModuleName, 14, "offence already slashed")
	ErrChallenge             = sdkerrors.Register(ModuleName, 15, "invalid challenge action")
	ErrAvailability          = sdkerrors.Register(ModuleName, 16, "invalid availability attestation")
	ErrParams                = sdkerrors.Register(ModuleName, 17, "invalid params")
	ErrNotFinalizable        = sdkerrors.Register(ModuleName, 18, "checkpoint not finalizable")
)
