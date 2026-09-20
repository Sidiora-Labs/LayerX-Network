package types

import (
	"fmt"

	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
)

func DefaultGenesis() *GenesisState {
	return &GenesisState{Params: DefaultParams()}
}

// Validate checks one asset mapping.
func (a AssetMapping) Validate() error {
	if _, err := ParseNonZeroHash32(a.AssetId); err != nil {
		return err
	}
	if err := sdk.ValidateDenom(a.Denom); err != nil {
		return sdkerrors.Wrap(ErrInvalidAsset, err.Error())
	}
	if a.Pointer != "" {
		if _, err := ParseAddress(a.Pointer); err != nil {
			return err
		}
	}
	if _, err := a.Minimum(); err != nil {
		return err
	}
	_, _, err := a.Cap()
	return err
}

// Minimum is the smallest accepted deposit; empty means zero.
func (a AssetMapping) Minimum() (sdk.Int, error) {
	if a.MinimumDeposit == "" {
		return sdk.ZeroInt(), nil
	}
	return ParseAmount(a.MinimumDeposit)
}

// Cap is the largest custodied total; the boolean is false when uncapped.
func (a AssetMapping) Cap() (sdk.Int, bool, error) {
	if a.CustodyCap == "" {
		return sdk.Int{}, false, nil
	}
	value, err := ParseAmount(a.CustodyCap)
	return value, err == nil, err
}

func (c Checkpoint) Validate() error {
	if _, err := ParseNonZeroHash32(c.StateRoot); err != nil {
		return err
	}
	if _, err := ParseNonZeroHash32(c.ReceiptRoot); err != nil {
		return err
	}
	if c.FinalizedAt < 0 {
		return sdkerrors.Wrap(ErrInvalidCheckpoint, "negative finalization time")
	}
	return nil
}

func (c Claim) Validate() error {
	for _, text := range []string{c.ClaimId, c.Nullifier, c.WithdrawalId, c.Account, c.AssetId, c.Anchor} {
		if _, err := ParseNonZeroHash32(text); err != nil {
			return err
		}
	}
	if _, err := ParseAddress(c.Recipient); err != nil {
		return err
	}
	amount, err := ParseAmount(c.Amount)
	if err != nil {
		return err
	}
	if !amount.IsPositive() || sdk.ValidateDenom(c.Denom) != nil {
		return sdkerrors.Wrap(ErrInvalidClaim, "amount or denom")
	}
	if c.Kind != ClaimKind_CLAIM_KIND_WITHDRAWAL && c.Kind != ClaimKind_CLAIM_KIND_FORCED_EXIT {
		return sdkerrors.Wrap(ErrInvalidClaim, "kind")
	}
	if c.Status < ClaimStatus_CLAIM_STATUS_PENDING || c.Status > ClaimStatus_CLAIM_STATUS_CANCELLED {
		return sdkerrors.Wrap(ErrInvalidClaim, "status")
	}
	return nil
}

func (d Deposit) Validate() error {
	for _, text := range []string{d.DepositId, d.Beneficiary, d.AssetId} {
		if _, err := ParseNonZeroHash32(text); err != nil {
			return err
		}
	}
	if _, err := ParseAddress(d.Depositor); err != nil {
		return err
	}
	amount, err := ParseAmount(d.Amount)
	if err != nil {
		return err
	}
	if !amount.IsPositive() || sdk.ValidateDenom(d.Denom) != nil || d.Index == 0 || d.Nonce == 0 {
		return sdkerrors.Wrap(ErrInvalidDeposit, "amount, denom, index or nonce")
	}
	return nil
}

// Validate performs stateless genesis validation, including that every
// pending claim holds a reserved nullifier and every other claim a terminal
// one: a genesis can never re-open a paid withdrawal.
func (gs GenesisState) Validate() error {
	if err := gs.Params.Validate(); err != nil {
		return err
	}
	fail := func(format string, args ...interface{}) error {
		return sdkerrors.Wrap(ErrInvalidGenesis, fmt.Sprintf(format, args...))
	}
	assets := map[string]bool{}
	pointers := map[string]bool{}
	for _, asset := range gs.Assets {
		if err := asset.Validate(); err != nil {
			return err
		}
		if assets[asset.AssetId] || (asset.Pointer != "" && pointers[asset.Pointer]) {
			return fail("duplicate asset %s", asset.AssetId)
		}
		assets[asset.AssetId] = true
		pointers[asset.Pointer] = true
	}
	deposits := map[string]bool{}
	indexes := map[uint64]bool{}
	for _, deposit := range gs.Deposits {
		if err := deposit.Validate(); err != nil {
			return err
		}
		if deposits[deposit.DepositId] || indexes[deposit.Index] || deposit.Index > gs.DepositCount {
			return fail("deposit %s index %d", deposit.DepositId, deposit.Index)
		}
		deposits[deposit.DepositId] = true
		indexes[deposit.Index] = true
	}
	for _, nonce := range gs.DepositNonces {
		if _, err := ParseAddress(nonce.Depositor); err != nil {
			return err
		}
		if _, err := ParseNonZeroHash32(nonce.AssetId); err != nil {
			return err
		}
	}
	nullifiers := map[string]Nullifier{}
	withdrawalIDs := map[string]bool{}
	for _, nullifier := range gs.Nullifiers {
		for _, text := range []string{nullifier.Nullifier, nullifier.ClaimId, nullifier.WithdrawalId} {
			if _, err := ParseNonZeroHash32(text); err != nil {
				return err
			}
		}
		if nullifier.Status < NullifierStatus_NULLIFIER_STATUS_RESERVED || nullifier.Status > NullifierStatus_NULLIFIER_STATUS_CANCELLED {
			return fail("nullifier %s status", nullifier.Nullifier)
		}
		if _, seen := nullifiers[nullifier.Nullifier]; seen || withdrawalIDs[nullifier.WithdrawalId] {
			return fail("duplicate nullifier %s", nullifier.Nullifier)
		}
		nullifiers[nullifier.Nullifier] = nullifier
		withdrawalIDs[nullifier.WithdrawalId] = true
	}
	claims := map[string]bool{}
	for _, claim := range gs.Claims {
		if err := claim.Validate(); err != nil {
			return err
		}
		if claims[claim.ClaimId] {
			return fail("duplicate claim %s", claim.ClaimId)
		}
		claims[claim.ClaimId] = true
		nullifier, ok := nullifiers[claim.Nullifier]
		if !ok || nullifier.ClaimId != claim.ClaimId || nullifier.WithdrawalId != claim.WithdrawalId ||
			int32(nullifier.Status) != int32(claim.Status) {
			return fail("claim %s is not bound to a nullifier of the same status", claim.ClaimId)
		}
	}
	if len(claims) != len(nullifiers) {
		return fail("%d claims for %d nullifiers", len(claims), len(nullifiers))
	}
	batches := map[uint64]bool{}
	for _, checkpoint := range gs.Checkpoints {
		if err := checkpoint.Validate(); err != nil {
			return err
		}
		if batches[checkpoint.BatchNumber] {
			return fail("duplicate checkpoint %d", checkpoint.BatchNumber)
		}
		batches[checkpoint.BatchNumber] = true
	}
	roots := map[string]bool{}
	for _, registration := range gs.DepositRoots {
		for _, text := range []string{registration.CheckpointId, registration.DepositRoot, registration.Commitment} {
			if _, err := ParseNonZeroHash32(text); err != nil {
				return err
			}
		}
		if roots[registration.CheckpointId] {
			return fail("duplicate deposit root for checkpoint %s", registration.CheckpointId)
		}
		roots[registration.CheckpointId] = true
	}
	for _, consumed := range gs.ConsumedBalances {
		for _, text := range []string{consumed.Account, consumed.AssetId, consumed.Anchor} {
			if _, err := ParseNonZeroHash32(text); err != nil {
				return err
			}
		}
	}
	for _, totals := range gs.Totals {
		if _, err := ParseNonZeroHash32(totals.AssetId); err != nil {
			return err
		}
		for _, text := range []string{totals.Custodied, totals.Released, totals.Pending} {
			if _, err := ParseAmount(text); err != nil {
				return err
			}
		}
	}
	return nil
}
