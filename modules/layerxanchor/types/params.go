package types

import (
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// AnchorPrecompileAddress is the settlement address guarantors attest to.
var AnchorPrecompileAddress = Address20{18: 0x10, 19: 0x14}

type Params struct {
	// Authority arbitrates challenges, activates guarantors, maintains
	// sequencer authorizations and updates params. It defaults to the gov
	// module account.
	Authority string `json:"authority"`
	BondDenom string `json:"bond_denom"`
	// Threshold is the guarantor quorum; a certificate must declare it.
	Threshold uint32  `json:"threshold"`
	MinBond   sdk.Int `json:"min_bond"`
	// UnbondingDelaySeconds keeps leaving bond slashable.
	UnbondingDelaySeconds uint64 `json:"unbonding_delay_seconds"`
	// ChallengeWindowSeconds must elapse after submission before finality.
	ChallengeWindowSeconds     uint64    `json:"challenge_window_seconds"`
	ChallengeBond              sdk.Int   `json:"challenge_bond"`
	MaxAttestationDelayMs      uint64    `json:"max_attestation_delay_ms"`
	PaxeerChainID              uint64    `json:"paxeer_chain_id"`
	SettlementContract         Address20 `json:"settlement_contract"`
	NetworkID                  uint32    `json:"network_id"`
	SlashFractionEquivocation  sdk.Dec   `json:"slash_fraction_equivocation"`
	SlashFractionFraud         sdk.Dec   `json:"slash_fraction_fraud"`
	SlashFractionAvailability  sdk.Dec   `json:"slash_fraction_availability"`
	ReporterShare              sdk.Dec   `json:"reporter_share"`
	SlashDestination           string    `json:"slash_destination"`
	PermissionlessRegistration bool      `json:"permissionless_registration"`
}

func DefaultParams(authority string) Params {
	return Params{
		Authority:                 authority,
		BondDenom:                 sdk.DefaultBondDenom,
		Threshold:                 1,
		MinBond:                   sdk.NewInt(1_000_000),
		UnbondingDelaySeconds:     21 * 24 * 60 * 60,
		ChallengeWindowSeconds:    0,
		ChallengeBond:             sdk.NewInt(1_000_000),
		MaxAttestationDelayMs:     3_600_000,
		PaxeerChainID:             0,
		SettlementContract:        AnchorPrecompileAddress,
		SlashFractionEquivocation: sdk.OneDec(),
		SlashFractionFraud:        sdk.OneDec(),
		SlashFractionAvailability: sdk.NewDecWithPrec(5, 1),
		ReporterShare:             sdk.NewDecWithPrec(1, 1),
	}
}

func validFraction(d sdk.Dec) bool {
	return !d.IsNil() && !d.IsNegative() && d.LTE(sdk.OneDec())
}

func (p Params) Validate() error {
	if _, err := sdk.AccAddressFromBech32(p.Authority); err != nil {
		return ErrParams.Wrapf("authority: %s", err)
	}
	if err := sdk.ValidateDenom(p.BondDenom); err != nil {
		return ErrParams.Wrap(err.Error())
	}
	if p.Threshold == 0 || p.Threshold > 32 {
		return ErrParams.Wrap("threshold must be within 1..32")
	}
	if p.MinBond.IsNil() || !p.MinBond.IsPositive() {
		return ErrParams.Wrap("min bond must be positive")
	}
	if p.ChallengeBond.IsNil() || p.ChallengeBond.IsNegative() {
		return ErrParams.Wrap("challenge bond must not be negative")
	}
	if p.MaxAttestationDelayMs == 0 {
		return ErrParams.Wrap("attestation delay must be positive")
	}
	if p.SettlementContract == (Address20{}) {
		return ErrParams.Wrap("settlement contract must be set")
	}
	if !validFraction(p.SlashFractionEquivocation) || !validFraction(p.SlashFractionFraud) ||
		!validFraction(p.SlashFractionAvailability) || !validFraction(p.ReporterShare) {
		return ErrParams.Wrap("fractions must be within 0..1")
	}
	if p.SlashDestination != "" {
		if _, err := sdk.AccAddressFromBech32(p.SlashDestination); err != nil {
			return ErrParams.Wrapf("slash destination: %s", err)
		}
	}
	return nil
}
