package keeper

import (
	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/layerxproof/codec"
	"github.com/sidiora-labs/paxeer-network/layerxproof/verify"
	"github.com/sidiora-labs/paxeer-network/modules/layerxcustody/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
)

// WithdrawalEvidence is a LayerX withdrawal receipt, its inclusion proof and
// the sequencer-signed batch header it is included in. Nothing in it is
// trusted: every byte is verified against AnchorReader material.
type WithdrawalEvidence struct {
	Receipt         []byte
	Proof           []byte
	Header          []byte
	HeaderSignature []byte
}

// ExitEvidence is a forced-exit request: a native state witness proving the
// account under the latest finalized state root, and the account authority's
// signature naming the Paxeer recipient.
type ExitEvidence struct {
	Witness            []byte
	BatchNumber        uint64
	Account            [32]byte
	AssetID            [32]byte
	Recipient          common.Address
	RecipientSignature []byte
}

// ClaimResult reports a claim and whether this call queued it.
type ClaimResult struct {
	Claim  types.Claim
	Queued bool
}

func (k *Keeper) GetClaim(ctx sdk.Context, claimID [32]byte) (types.Claim, bool) {
	var claim types.Claim
	bz := k.store(ctx).Get(types.ClaimKey(claimID))
	if bz == nil {
		return claim, false
	}
	k.cdc.MustUnmarshal(bz, &claim)
	return claim, true
}

func (k *Keeper) setClaim(ctx sdk.Context, claim types.Claim) {
	claimID, _ := types.ParseHash32(claim.ClaimId)
	k.store(ctx).Set(types.ClaimKey(claimID), k.cdc.MustMarshal(&claim))
}

func (k *Keeper) IterateClaims(ctx sdk.Context, visit func(types.Claim) bool) {
	k.iterate(ctx, types.ClaimPrefix, func(value []byte) bool {
		var claim types.Claim
		k.cdc.MustUnmarshal(value, &claim)
		return visit(claim)
	})
}

func (k *Keeper) GetNullifier(ctx sdk.Context, nullifier [32]byte) (types.Nullifier, bool) {
	var record types.Nullifier
	bz := k.store(ctx).Get(types.NullifierKey(nullifier))
	if bz == nil {
		return record, false
	}
	k.cdc.MustUnmarshal(bz, &record)
	return record, true
}

func (k *Keeper) setNullifier(ctx sdk.Context, record types.Nullifier) {
	nullifier, _ := types.ParseHash32(record.Nullifier)
	withdrawalID, _ := types.ParseHash32(record.WithdrawalId)
	k.store(ctx).Set(types.NullifierKey(nullifier), k.cdc.MustMarshal(&record))
	k.store(ctx).Set(types.WithdrawalIDKey(withdrawalID), []byte{1})
}

func (k *Keeper) IterateNullifiers(ctx sdk.Context, visit func(types.Nullifier) bool) {
	k.iterate(ctx, types.NullifierPrefix, func(value []byte) bool {
		var record types.Nullifier
		k.cdc.MustUnmarshal(value, &record)
		return visit(record)
	})
}

func (k *Keeper) WithdrawalIDUsed(ctx sdk.Context, withdrawalID [32]byte) bool {
	return k.store(ctx).Has(types.WithdrawalIDKey(withdrawalID))
}

func (k *Keeper) BalanceConsumed(ctx sdk.Context, account, assetID, anchor [32]byte) bool {
	return k.store(ctx).Has(types.ConsumedKey(account, assetID, anchor))
}

// transition moves a reserved nullifier to a terminal status, exactly
// WithdrawalNullifierRegistry._transition.
func (k *Keeper) transition(ctx sdk.Context, claim types.Claim, next types.NullifierStatus) error {
	nullifier, _ := types.ParseHash32(claim.Nullifier)
	record, found := k.GetNullifier(ctx, nullifier)
	if !found || record.Status != types.NullifierStatus_NULLIFIER_STATUS_RESERVED || record.ClaimId != claim.ClaimId {
		return types.ErrNullifierUsed
	}
	record.Status = next
	k.setNullifier(ctx, record)
	return nil
}

type pendingClaim struct {
	kind         types.ClaimKind
	claimID      [32]byte
	nullifier    [32]byte
	withdrawalID [32]byte
	account      [32]byte
	assetID      [32]byte
	anchor       [32]byte
	recipient    common.Address
	amount       codec.U128
	batchNumber  uint64
	delay        uint64
}

// queue reserves the nullifier and records a pending claim.
func (k *Keeper) queue(ctx sdk.Context, p pendingClaim) (types.Claim, error) {
	if _, used := k.GetNullifier(ctx, p.nullifier); used || k.WithdrawalIDUsed(ctx, p.withdrawalID) {
		return types.Claim{}, types.ErrNullifierUsed
	}
	if _, exists := k.GetClaim(ctx, p.claimID); exists {
		return types.Claim{}, sdkerrors.Wrap(types.ErrInvalidClaim, "claim already exists")
	}
	asset, found := k.GetAsset(ctx, p.assetID)
	if !found {
		return types.Claim{}, types.ErrUnknownAsset
	}
	amount := sdk.NewIntFromBigInt(types.AmountInt(p.amount))
	if !amount.IsPositive() || p.recipient == (common.Address{}) {
		return types.Claim{}, sdkerrors.Wrap(types.ErrInvalidClaim, "amount or recipient")
	}
	claim := types.Claim{
		ClaimId:      types.Hash32(p.claimID),
		Kind:         p.kind,
		Status:       types.ClaimStatus_CLAIM_STATUS_PENDING,
		Nullifier:    types.Hash32(p.nullifier),
		WithdrawalId: types.Hash32(p.withdrawalID),
		Account:      types.Hash32(p.account),
		AssetId:      asset.AssetId,
		Denom:        asset.Denom,
		Recipient:    types.Address(p.recipient),
		Amount:       amount.String(),
		BatchNumber:  p.batchNumber,
		Anchor:       types.Hash32(p.anchor),
		AvailableAt:  ctx.BlockTime().Unix() + int64(p.delay), //nolint:gosec
	}
	k.setClaim(ctx, claim)
	k.setNullifier(ctx, types.Nullifier{Nullifier: claim.Nullifier, Status: types.NullifierStatus_NULLIFIER_STATUS_RESERVED,
		ClaimId: claim.ClaimId, WithdrawalId: claim.WithdrawalId})
	custodied, released, pending := k.totals(ctx, p.assetID)
	k.setTotals(ctx, p.assetID, custodied, released, pending.Add(amount))
	return claim, ctx.EventManager().EmitTypedEvent(&types.EventClaimQueued{
		ClaimId: claim.ClaimId, Nullifier: claim.Nullifier, Kind: claim.Kind.String(), BatchNumber: claim.BatchNumber,
		AssetId: claim.AssetId, Recipient: claim.Recipient, Amount: claim.Amount, AvailableAt: claim.AvailableAt,
	})
}

// pay consumes the nullifier and releases the claim from the module account.
// It is the only function that moves coins out of custody.
func (k *Keeper) pay(ctx sdk.Context, claim types.Claim) (types.Claim, error) {
	if claim.Status != types.ClaimStatus_CLAIM_STATUS_PENDING {
		return claim, types.ErrClaimNotPending
	}
	if ctx.BlockTime().Unix() < claim.AvailableAt {
		return claim, sdkerrors.Wrapf(types.ErrClaimNotReady, "available at %d", claim.AvailableAt)
	}
	assetID, _ := types.ParseHash32(claim.AssetId)
	asset, found := k.GetAsset(ctx, assetID)
	if !found || !asset.Enabled || asset.Paused || asset.Denom != claim.Denom {
		return claim, types.ErrAssetDisabled
	}
	amount, _ := sdk.NewIntFromString(claim.Amount)
	custodied, released, pending := k.totals(ctx, assetID)
	balance := k.bankKeeper.GetBalance(ctx, k.ModuleAddress(), claim.Denom).Amount
	if amount.GT(custodied) || amount.GT(balance) || amount.GT(pending) {
		return claim, sdkerrors.Wrap(types.ErrInvalidRelease, "claim exceeds custody")
	}
	if err := k.transition(ctx, claim, types.NullifierStatus_NULLIFIER_STATUS_CONSUMED); err != nil {
		return claim, err
	}
	claim.Status = types.ClaimStatus_CLAIM_STATUS_PAID
	k.setClaim(ctx, claim)
	k.setTotals(ctx, assetID, custodied.Sub(amount), released.Add(amount), pending.Sub(amount))
	recipient, _ := types.ParseAddress(claim.Recipient)
	if err := k.bankKeeper.SendCoinsFromModuleToAccount(ctx, types.ModuleName,
		k.evmKeeper.GetPaxAddressOrDefault(ctx, recipient), sdk.NewCoins(sdk.NewCoin(claim.Denom, amount))); err != nil {
		return claim, err
	}
	if claim.Kind == types.ClaimKind_CLAIM_KIND_FORCED_EXIT {
		if err := ctx.EventManager().EmitTypedEvent(&types.EventForcedExitExecuted{ClaimId: claim.ClaimId,
			Nullifier: claim.Nullifier, Anchor: claim.Anchor, Account: claim.Account, AssetId: claim.AssetId,
			Recipient: claim.Recipient, Amount: claim.Amount}); err != nil {
			return claim, err
		}
	} else if err := ctx.EventManager().EmitTypedEvent(&types.EventClaimFinalised{ClaimId: claim.ClaimId,
		Nullifier: claim.Nullifier}); err != nil {
		return claim, err
	}
	return claim, ctx.EventManager().EmitTypedEvent(&types.EventCustodyRelease{ClaimId: claim.ClaimId,
		AssetId: claim.AssetId, Recipient: claim.Recipient, Amount: claim.Amount, Denom: claim.Denom})
}

// atomically runs a state transition on a branch that is committed, with its
// events, only when the whole transition succeeds.
func atomically[T any](ctx sdk.Context, run func(sdk.Context) (T, error)) (T, error) {
	cached, write := ctx.CacheContext()
	out, err := run(cached)
	if err != nil {
		var zero T
		return zero, err
	}
	write()
	ctx.EventManager().EmitEvents(cached.EventManager().Events())
	return out, nil
}

// VerifyWithdrawal proves evidence against AnchorReader material: the batch
// must have an authorised sequencer and be a finalized checkpoint whose roots
// equal the signed header's, the receipt must be included under the header's
// receipt root and signed by the sequencer, and it must be an exact successful
// native withdrawal on this module's LayerX network.
func (k *Keeper) VerifyWithdrawal(ctx sdk.Context, evidence WithdrawalEvidence) (*verify.WithdrawalEffect, *codec.BatchHeader, error) {
	if len(evidence.HeaderSignature) != 64 {
		return nil, nil, sdkerrors.Wrap(types.ErrInvalidProof, "header signature must be 64 bytes")
	}
	var signature [64]byte
	copy(signature[:], evidence.HeaderSignature)
	header, err := codec.DecodeBatchHeader(evidence.Header)
	if err != nil {
		return nil, nil, sdkerrors.Wrap(types.ErrInvalidProof, err.Error())
	}
	params := k.GetParams(ctx)
	if params.NetworkId == 0 || header.NetworkID != params.NetworkId {
		return nil, nil, types.ErrWrongNetwork
	}
	anchor := k.Anchor()
	authorization, ok := anchor.SequencerAuthorization(ctx, header.BatchNumber)
	if !ok {
		return nil, nil, types.ErrNotAuthorized
	}
	stateRoot, stateOK := anchor.FinalizedStateRoot(ctx, header.BatchNumber)
	receiptRoot, receiptOK := anchor.FinalizedReceiptRoot(ctx, header.BatchNumber)
	if !stateOK || !receiptOK || stateRoot != header.ResultingStateRoot || receiptRoot != header.ReceiptMerkleRoot {
		return nil, nil, types.ErrNotFinalized
	}
	proof, err := codec.DecodeMerkleProof(evidence.Proof)
	if err != nil {
		return nil, nil, sdkerrors.Wrap(types.ErrInvalidProof, err.Error())
	}
	_, verified, effect, err := verify.WithdrawalInclusion(evidence.Receipt, proof, evidence.Header, signature, authorization)
	if err != nil {
		return nil, nil, sdkerrors.Wrap(types.ErrInvalidProof, err.Error())
	}
	if effect.NetworkID != params.NetworkId ||
		effect.Nullifier != types.WithdrawalNullifier(effect.NetworkID, effect.WithdrawalID, effect.Account, effect.Asset, effect.Amount, effect.Anchor) {
		return nil, nil, types.ErrWrongNetwork
	}
	return effect, verified.Header, nil
}

func (k *Keeper) withdrawalClaim(ctx sdk.Context, evidence WithdrawalEvidence) (pendingClaim, error) {
	effect, header, err := k.VerifyWithdrawal(ctx, evidence)
	if err != nil {
		return pendingClaim{}, err
	}
	recipient := common.Address(effect.Recipient)
	return pendingClaim{
		kind:         types.ClaimKind_CLAIM_KIND_WITHDRAWAL,
		claimID:      types.WithdrawalClaimID(k.evmKeeper.ChainID(ctx), effect.Nullifier, recipient),
		nullifier:    effect.Nullifier,
		withdrawalID: effect.WithdrawalID,
		account:      effect.Account,
		assetID:      effect.Asset,
		anchor:       effect.Anchor,
		recipient:    recipient,
		amount:       effect.Amount,
		batchNumber:  header.BatchNumber,
		delay:        k.GetParams(ctx).WithdrawalDelaySeconds,
	}, nil
}

// RequestWithdrawal verifies the evidence, reserves the nullifier and queues
// the claim; it becomes payable after the withdrawal delay.
func (k *Keeper) RequestWithdrawal(ctx sdk.Context, evidence WithdrawalEvidence) (types.Claim, error) {
	return atomically(ctx, func(ctx sdk.Context) (types.Claim, error) {
		pending, err := k.withdrawalClaim(ctx, evidence)
		if err != nil {
			return types.Claim{}, err
		}
		return k.queue(ctx, pending)
	})
}

func (k *Keeper) finalise(ctx sdk.Context, pending pendingClaim) (ClaimResult, error) {
	return atomically(ctx, func(ctx sdk.Context) (ClaimResult, error) {
		claim, found := k.GetClaim(ctx, pending.claimID)
		if !found {
			queued, err := k.queue(ctx, pending)
			if err != nil {
				return ClaimResult{}, err
			}
			claim = queued
		}
		paid, err := k.pay(ctx, claim)
		return ClaimResult{Claim: paid, Queued: !found}, err
	})
}

// FinaliseWithdrawal re-verifies the evidence against the current anchor
// material and pays the claim to the recipient the receipt names. A claim
// that was never requested is queued first, which succeeds in one call only
// when the withdrawal delay is zero.
func (k *Keeper) FinaliseWithdrawal(ctx sdk.Context, evidence WithdrawalEvidence) (ClaimResult, error) {
	pending, err := k.withdrawalClaim(ctx, evidence)
	if err != nil {
		return ClaimResult{}, err
	}
	return k.finalise(ctx, pending)
}

// ExitEligible is EmergencyExit.eligible: a finalized checkpoint exists and
// either the authority declared an emergency or no checkpoint was finalized
// within the liveness bound.
func (k *Keeper) ExitEligible(ctx sdk.Context) bool {
	_, finalizedAt, ok := k.Anchor().LatestFinalizedBatch(ctx)
	if !ok {
		return false
	}
	return k.GetEmergency(ctx) ||
		ctx.BlockTime().Unix() >= finalizedAt+int64(k.GetParams(ctx).LivenessBoundSeconds) //nolint:gosec
}

// VerifyForcedExit proves the whole account balance under the latest
// finalized state root. The anchor of a forced exit is that state root: it is
// the request anchor of the authority's recipient signature, of the required
// withdrawal identifier and of the nullifier.
func (k *Keeper) VerifyForcedExit(ctx sdk.Context, evidence ExitEvidence) (pendingClaim, error) {
	if !k.ExitEligible(ctx) {
		return pendingClaim{}, types.ErrExitNotEligible
	}
	params := k.GetParams(ctx)
	latest, _, _ := k.Anchor().LatestFinalizedBatch(ctx)
	stateRoot, ok := k.Anchor().FinalizedStateRoot(ctx, evidence.BatchNumber)
	if !ok || evidence.BatchNumber != latest {
		return pendingClaim{}, types.ErrNotFinalized
	}
	if len(evidence.RecipientSignature) != 64 || params.NetworkId == 0 {
		return pendingClaim{}, sdkerrors.Wrap(types.ErrInvalidExit, "recipient signature or network")
	}
	var signature [64]byte
	copy(signature[:], evidence.RecipientSignature)
	account, err := verify.ExitBalance(evidence.Witness, stateRoot, params.NetworkId, evidence.Account,
		evidence.AssetID, evidence.Recipient, stateRoot, signature)
	if err != nil {
		return pendingClaim{}, sdkerrors.Wrap(types.ErrInvalidProof, err.Error())
	}
	if account.Balance.IsZero() {
		return pendingClaim{}, sdkerrors.Wrap(types.ErrInvalidExit, "zero balance")
	}
	withdrawalID := types.ExitWithdrawalID(params.NetworkId, evidence.Account, evidence.AssetID, stateRoot)
	nullifier := types.WithdrawalNullifier(params.NetworkId, withdrawalID, evidence.Account, evidence.AssetID, account.Balance, stateRoot)
	return pendingClaim{
		kind:         types.ClaimKind_CLAIM_KIND_FORCED_EXIT,
		claimID:      types.ExitClaimID(k.evmKeeper.ChainID(ctx), nullifier),
		nullifier:    nullifier,
		withdrawalID: withdrawalID,
		account:      evidence.Account,
		assetID:      evidence.AssetID,
		anchor:       stateRoot,
		recipient:    evidence.Recipient,
		amount:       account.Balance,
		batchNumber:  evidence.BatchNumber,
		delay:        params.ForcedExitDelaySeconds,
	}, nil
}

func (k *Keeper) queueExit(ctx sdk.Context, pending pendingClaim) (types.Claim, error) {
	if k.BalanceConsumed(ctx, pending.account, pending.assetID, pending.anchor) {
		return types.Claim{}, types.ErrExitConsumed
	}
	claim, err := k.queue(ctx, pending)
	if err != nil {
		return types.Claim{}, err
	}
	k.store(ctx).Set(types.ConsumedKey(pending.account, pending.assetID, pending.anchor), []byte{1})
	return claim, nil
}

// RequestForcedExit verifies the evidence, marks the balance consumed at its
// anchor and queues the exit; it becomes payable after the forced-exit delay.
func (k *Keeper) RequestForcedExit(ctx sdk.Context, evidence ExitEvidence) (types.Claim, error) {
	return atomically(ctx, func(ctx sdk.Context) (types.Claim, error) {
		pending, err := k.VerifyForcedExit(ctx, evidence)
		if err != nil {
			return types.Claim{}, err
		}
		return k.queueExit(ctx, pending)
	})
}

// ExecuteForcedExit pays a forced exit. An exit that was requested earlier is
// identified by re-deriving its claim from the same evidence, which is
// re-verified only while the exit is still against the latest checkpoint; a
// queued exit stays payable after newer checkpoints are finalized.
func (k *Keeper) ExecuteForcedExit(ctx sdk.Context, evidence ExitEvidence) (ClaimResult, error) {
	return atomically(ctx, func(ctx sdk.Context) (ClaimResult, error) {
		if claim, found := k.queuedExit(ctx, evidence); found {
			paid, err := k.pay(ctx, claim)
			return ClaimResult{Claim: paid}, err
		}
		pending, err := k.VerifyForcedExit(ctx, evidence)
		if err != nil {
			return ClaimResult{}, err
		}
		claim, err := k.queueExit(ctx, pending)
		if err != nil {
			return ClaimResult{}, err
		}
		paid, err := k.pay(ctx, claim)
		return ClaimResult{Claim: paid, Queued: true}, err
	})
}

// queuedExit finds the claim an earlier RequestForcedExit queued for the same
// account, asset, batch and recipient. The claim was fully verified when it
// was queued and its recipient is part of the stored record.
func (k *Keeper) queuedExit(ctx sdk.Context, evidence ExitEvidence) (types.Claim, bool) {
	stateRoot, ok := k.Anchor().FinalizedStateRoot(ctx, evidence.BatchNumber)
	if !ok || !k.BalanceConsumed(ctx, evidence.Account, evidence.AssetID, stateRoot) {
		return types.Claim{}, false
	}
	var out types.Claim
	found := false
	k.IterateClaims(ctx, func(claim types.Claim) bool {
		if claim.Kind == types.ClaimKind_CLAIM_KIND_FORCED_EXIT && claim.Account == types.Hash32(evidence.Account) &&
			claim.AssetId == types.Hash32(evidence.AssetID) && claim.Anchor == types.Hash32(stateRoot) &&
			claim.Recipient == types.Address(evidence.Recipient) {
			out, found = claim, true
		}
		return found
	})
	return out, found
}

// CancelClaim is the challenge outcome: the authority cancels a pending claim
// inside its window. The nullifier becomes terminally cancelled, exactly as
// WithdrawalClaims.cancelChallengedClaim leaves it.
func (k *Keeper) CancelClaim(ctx sdk.Context, claimID [32]byte) (types.Claim, error) {
	return atomically(ctx, func(ctx sdk.Context) (types.Claim, error) {
		claim, found := k.GetClaim(ctx, claimID)
		if !found || claim.Status != types.ClaimStatus_CLAIM_STATUS_PENDING {
			return types.Claim{}, types.ErrClaimNotPending
		}
		if err := k.transition(ctx, claim, types.NullifierStatus_NULLIFIER_STATUS_CANCELLED); err != nil {
			return types.Claim{}, err
		}
		claim.Status = types.ClaimStatus_CLAIM_STATUS_CANCELLED
		k.setClaim(ctx, claim)
		assetID, _ := types.ParseHash32(claim.AssetId)
		amount, _ := sdk.NewIntFromString(claim.Amount)
		custodied, released, pending := k.totals(ctx, assetID)
		k.setTotals(ctx, assetID, custodied, released, pending.Sub(amount))
		return claim, ctx.EventManager().EmitTypedEvent(&types.EventClaimCancelled{ClaimId: claim.ClaimId, Nullifier: claim.Nullifier})
	})
}
