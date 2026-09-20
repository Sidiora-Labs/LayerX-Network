package keeper

import (
	"bytes"
	"encoding/json"
	"fmt"

	"github.com/sidiora-labs/paxeer-network/layerxproof/codec"
	"github.com/sidiora-labs/paxeer-network/layerxproof/verify"
	"github.com/sidiora-labs/paxeer-network/modules/layerxanchor/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

func (k Keeper) GetCheckpoint(ctx sdk.Context, batchNumber uint64) (types.Checkpoint, bool) {
	var checkpoint types.Checkpoint
	ok := k.get(ctx, types.CheckpointKey(batchNumber), &checkpoint)
	return checkpoint, ok
}

func (k Keeper) setCheckpoint(ctx sdk.Context, checkpoint types.Checkpoint) {
	k.set(ctx, types.CheckpointKey(checkpoint.BatchNumber), checkpoint)
	k.set(ctx, types.CheckpointIDKey(checkpoint.CheckpointID), checkpoint.BatchNumber)
}

func (k Keeper) deleteCheckpoint(ctx sdk.Context, checkpoint types.Checkpoint) {
	store := ctx.KVStore(k.storeKey)
	store.Delete(types.CheckpointKey(checkpoint.BatchNumber))
	store.Delete(types.CheckpointIDKey(checkpoint.CheckpointID))
}

// CheckpointByID is the recorded checkpoint carrying the identifier. A removed
// checkpoint, or a batch since recorded under another identifier, is unknown.
func (k Keeper) CheckpointByID(ctx sdk.Context, checkpointID [32]byte) (types.Checkpoint, bool) {
	var batchNumber uint64
	if !k.get(ctx, types.CheckpointIDKey(checkpointID), &batchNumber) {
		return types.Checkpoint{}, false
	}
	checkpoint, ok := k.GetCheckpoint(ctx, batchNumber)
	if !ok || checkpoint.CheckpointID != types.Hash32(checkpointID) {
		return types.Checkpoint{}, false
	}
	return checkpoint, true
}

func (k Keeper) GetCheckpoints(ctx sdk.Context) []types.Checkpoint {
	var out []types.Checkpoint
	k.iterate(ctx, types.CheckpointPrefix, func(value []byte) bool {
		var checkpoint types.Checkpoint
		if err := json.Unmarshal(value, &checkpoint); err != nil {
			panic(err)
		}
		out = append(out, checkpoint)
		return false
	})
	return out
}

func (k Keeper) domain(params types.Params) verify.SettlementDomain {
	return verify.SettlementDomain{PaxeerChainID: params.PaxeerChainID, SettlementContract: params.SettlementContract}
}

// continuity requires the checkpoint to extend the latest finalized one (or
// the genesis anchor, or to be batch 1 when neither exists): next batch number, next global sequence, and the
// previous state root equal to the settled root, the settlement_anchor rule
// of lxp_checkpoint_finalisable.
func (k Keeper) continuity(ctx sdk.Context, header *codec.BatchHeader) error {
	if header.LastSequence < header.FirstSequence {
		return types.ErrContinuity.Wrap("reversed sequence range")
	}
	var batch, lastSequence uint64
	var root types.Hash32
	if latest, ok := k.LatestFinalizedBatch(ctx); ok {
		checkpoint, _ := k.GetCheckpoint(ctx, latest)
		batch, lastSequence, root = checkpoint.BatchNumber, checkpoint.LastSequence, checkpoint.StateRoot
	} else if anchor := k.GetAnchor(ctx); anchor.Set {
		batch, lastSequence, root = anchor.BatchNumber, anchor.LastSequence, anchor.StateRoot
	} else {
		// Without a genesis anchor the chain of checkpoints starts at batch 1.
		if header.BatchNumber != 1 {
			return types.ErrContinuity.Wrapf("batch %d cannot start the finalized chain", header.BatchNumber)
		}
		return nil
	}
	if header.BatchNumber != batch+1 {
		return types.ErrContinuity.Wrapf("batch %d does not follow finalized batch %d", header.BatchNumber, batch)
	}
	if header.FirstSequence != lastSequence+1 {
		return types.ErrContinuity.Wrapf("sequence %d does not follow %d", header.FirstSequence, lastSequence)
	}
	if types.Hash32(header.PreviousStateRoot) != root {
		return types.ErrContinuity.Wrap("previous state root is not the finalized state root")
	}
	return nil
}

// SubmitCheckpoint verifies a sequencer-signed header and its guarantor
// certificate, records the checkpoint as submitted, and finalizes it when the
// finality rule holds. Anyone may submit; every refusal leaves state unchanged.
func (k Keeper) SubmitCheckpoint(ctx sdk.Context, submitter sdk.AccAddress, header []byte, headerSignature [64]byte, certificateBytes []byte) (types.Checkpoint, error) {
	params := k.GetParams(ctx)
	certificate, err := codec.DecodeCheckpointCertificate(certificateBytes)
	if err != nil {
		return types.Checkpoint{}, types.ErrCertificate.Wrap(err.Error())
	}
	if !bytes.Equal(certificate.HeaderBytes, header) {
		return types.Checkpoint{}, types.ErrCertificate.Wrap("certificate is over a different header")
	}
	if params.NetworkID != 0 && certificate.Header.NetworkID != params.NetworkID {
		return types.Checkpoint{}, types.ErrCertificate.Wrap("network identifier")
	}
	authorization, ok := k.SequencerAuthorization(ctx, certificate.Header.SequencerID, certificate.Header.BatchNumber)
	if !ok {
		return types.Checkpoint{}, types.ErrSequencerUnauthorized
	}
	verified, err := verify.BatchHeader(header, headerSignature, authorization)
	if err != nil {
		return types.Checkpoint{}, types.ErrSequencerUnauthorized.Wrap(err.Error())
	}
	existing, exists := k.GetCheckpoint(ctx, certificate.Header.BatchNumber)
	if exists && existing.Status == types.CheckpointFinal {
		return types.Checkpoint{}, types.ErrCheckpointFinal
	}
	if err := k.continuity(ctx, certificate.Header); err != nil {
		return types.Checkpoint{}, err
	}
	checkpointID, signers, err := verify.CheckpointCertificate(certificate, k.domain(params), params.MaxAttestationDelayMs,
		k.EligibleSigner(ctx, params))
	if err != nil {
		return types.Checkpoint{}, types.ErrCertificate.Wrap(err.Error())
	}
	if exists {
		if existing.OpenChallenges != 0 {
			return types.Checkpoint{}, types.ErrChallenge.Wrap("the submitted checkpoint is under challenge")
		}
		if signers < len(existing.Guarantors) {
			return types.Checkpoint{}, types.ErrCertificate.Wrap("fewer guarantors than the submitted checkpoint")
		}
		if existing.CheckpointID != types.Hash32(checkpointID) {
			k.clearAvailability(ctx, existing.BatchNumber)
		}
	}
	h := certificate.Header
	checkpoint := types.Checkpoint{
		BatchNumber: h.BatchNumber, CheckpointID: checkpointID, HeaderDigest: verified.Digest,
		ProtocolVersion: h.ProtocolVersion, NetworkID: h.NetworkID, Epoch: h.Epoch,
		FirstSequence: h.FirstSequence, LastSequence: h.LastSequence,
		PreviousStateRoot: h.PreviousStateRoot, StateRoot: h.ResultingStateRoot, ReceiptRoot: h.ReceiptMerkleRoot,
		DataAvailabilityRoot: h.DataAvailabilityRoot, SequencerID: h.SequencerID, TimestampMs: h.TimestampMs,
		Status: types.CheckpointSubmitted, DeclaredThreshold: certificate.Threshold,
		Submitter: submitter.String(), SubmittedHeight: ctx.BlockHeight(), SubmittedTime: ctx.BlockTime().Unix(),
	}
	for _, attestation := range certificate.Attestations {
		checkpoint.Guarantors = append(checkpoint.Guarantors, attestation.GuarantorID)
		k.set(ctx, types.AvailabilityKey(h.BatchNumber, attestation.GuarantorID), types.AvailabilityAttestation{
			BatchNumber: h.BatchNumber, GuarantorID: attestation.GuarantorID, CheckpointID: checkpointID,
			ClassMask: attestation.AvailabilityClassMask, AttestedAtMs: attestation.AttestedAtMs, Height: ctx.BlockHeight(),
		})
	}
	checkpoint.AvailabilityMask = k.availabilityMask(ctx, checkpoint.BatchNumber, params)
	k.setCheckpoint(ctx, checkpoint)
	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventCheckpointSubmitted,
		sdk.NewAttribute(types.AttributeBatchNumber, fmt.Sprint(checkpoint.BatchNumber)),
		sdk.NewAttribute(types.AttributeCheckpointID, fmt.Sprintf("%x", checkpointID[:])),
		sdk.NewAttribute(types.AttributeStateRoot, fmt.Sprintf("%x", checkpoint.StateRoot[:])),
		sdk.NewAttribute(types.AttributeSigners, fmt.Sprint(signers))))
	if k.finalizable(ctx, checkpoint, params) == nil {
		checkpoint = k.finalize(ctx, checkpoint)
	}
	return checkpoint, nil
}

// finalizable is the finality rule ported from lxp_checkpoint_finalisable and
// CheckpointRegistry: the certificate declares exactly the required threshold,
// at least that many of its guarantors are still bonded and active (an
// equivocator was ejected), no challenge is open, the challenge window has
// elapsed, and the checkpoint continues the finalized chain.
func (k Keeper) finalizable(ctx sdk.Context, checkpoint types.Checkpoint, params types.Params) error {
	if checkpoint.Status != types.CheckpointSubmitted {
		return types.ErrNotFinalizable.Wrap("checkpoint is not in the submitted state")
	}
	if uint32(checkpoint.DeclaredThreshold) != params.Threshold {
		return types.ErrNotFinalizable.Wrap("certificate threshold is not the required threshold")
	}
	bonded := uint32(0)
	for _, id := range checkpoint.Guarantors {
		if guarantor, ok := k.GetGuarantor(ctx, id); ok && eligible(guarantor, params) {
			bonded++
		}
	}
	if bonded < params.Threshold {
		return types.ErrNotFinalizable.Wrap("bonded guarantor signatures are below the threshold")
	}
	if checkpoint.OpenChallenges != 0 {
		return types.ErrNotFinalizable.Wrap("a challenge is open")
	}
	if ctx.BlockTime().Unix() < checkpoint.SubmittedTime+int64(params.ChallengeWindowSeconds) { //nolint:gosec
		return types.ErrNotFinalizable.Wrap("the challenge window is open")
	}
	if latest, ok := k.LatestFinalizedBatch(ctx); ok && checkpoint.BatchNumber != latest+1 {
		return types.ErrNotFinalizable.Wrap("checkpoint does not follow the finalized batch")
	}
	return nil
}

func (k Keeper) finalize(ctx sdk.Context, checkpoint types.Checkpoint) types.Checkpoint {
	checkpoint.Status = types.CheckpointFinal
	checkpoint.FinalizedHeight = ctx.BlockHeight()
	checkpoint.FinalizedTime = ctx.BlockTime().Unix()
	k.setCheckpoint(ctx, checkpoint)
	k.set(ctx, types.LatestFinalizedKey, checkpoint.BatchNumber)
	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventCheckpointFinalized,
		sdk.NewAttribute(types.AttributeBatchNumber, fmt.Sprint(checkpoint.BatchNumber)),
		sdk.NewAttribute(types.AttributeCheckpointID, fmt.Sprintf("%x", checkpoint.CheckpointID[:])),
		sdk.NewAttribute(types.AttributeStateRoot, fmt.Sprintf("%x", checkpoint.StateRoot[:])),
		sdk.NewAttribute(types.AttributeReceiptRoot, fmt.Sprintf("%x", checkpoint.ReceiptRoot[:]))))
	return checkpoint
}

// Finalize finalizes a submitted checkpoint once the rule holds, for a
// checkpoint that waited out a challenge window or an open challenge.
func (k Keeper) Finalize(ctx sdk.Context, batchNumber uint64) (types.Checkpoint, error) {
	checkpoint, ok := k.GetCheckpoint(ctx, batchNumber)
	if !ok {
		return checkpoint, types.ErrCheckpointUnknown
	}
	if err := k.finalizable(ctx, checkpoint, k.GetParams(ctx)); err != nil {
		return checkpoint, err
	}
	return k.finalize(ctx, checkpoint), nil
}

// Availability.

func (k Keeper) GetAvailability(ctx sdk.Context) []types.AvailabilityAttestation {
	var out []types.AvailabilityAttestation
	k.iterate(ctx, types.AvailabilityPrefix, func(value []byte) bool {
		var attestation types.AvailabilityAttestation
		if err := json.Unmarshal(value, &attestation); err != nil {
			panic(err)
		}
		out = append(out, attestation)
		return false
	})
	return out
}

func (k Keeper) clearAvailability(ctx sdk.Context, batchNumber uint64) {
	for _, attestation := range k.GetAvailability(ctx) {
		if attestation.BatchNumber == batchNumber {
			ctx.KVStore(k.storeKey).Delete(types.AvailabilityKey(batchNumber, attestation.GuarantorID))
		}
	}
}

// availabilityMask is the set of the five classes that at least the threshold
// of bonded active guarantors attested to possess.
func (k Keeper) availabilityMask(ctx sdk.Context, batchNumber uint64, params types.Params) uint8 {
	var counts [codec.AvailabilityClassCount]uint32
	k.iterate(ctx, types.AvailabilityBatchPrefix(batchNumber), func(value []byte) bool {
		var attestation types.AvailabilityAttestation
		if err := json.Unmarshal(value, &attestation); err != nil {
			panic(err)
		}
		if guarantor, ok := k.GetGuarantor(ctx, attestation.GuarantorID); ok && eligible(guarantor, params) {
			for class := 0; class < codec.AvailabilityClassCount; class++ {
				if attestation.ClassMask&(1<<class) != 0 {
					counts[class]++
				}
			}
		}
		return false
	})
	mask := uint8(0)
	for class, count := range counts {
		if count >= params.Threshold {
			mask |= 1 << class
		}
	}
	return mask
}

// SubmitAvailabilityAttestation records one guarantor's signed possession
// statement for a known checkpoint. It must name that checkpoint, claim
// possession of a non-empty subset of the five classes, be fresh, and be
// signed by a bonded active guarantor. A later statement from the same
// guarantor may only widen its mask.
func (k Keeper) SubmitAvailabilityAttestation(ctx sdk.Context, encoded []byte) (types.Checkpoint, types.AvailabilityAttestation, error) {
	params := k.GetParams(ctx)
	attestation, err := codec.DecodeGuarantorAttestation(encoded)
	if err != nil {
		return types.Checkpoint{}, types.AvailabilityAttestation{}, types.ErrAvailability.Wrap(err.Error())
	}
	checkpoint, ok := k.GetCheckpoint(ctx, attestation.BatchNumber)
	if !ok {
		return checkpoint, types.AvailabilityAttestation{}, types.ErrCheckpointUnknown
	}
	header := &codec.BatchHeader{ProtocolVersion: checkpoint.ProtocolVersion, NetworkID: checkpoint.NetworkID,
		Epoch: checkpoint.Epoch, BatchNumber: checkpoint.BatchNumber, DataAvailabilityRoot: checkpoint.DataAvailabilityRoot,
		TimestampMs: checkpoint.TimestampMs}
	if err := verify.Attestation(attestation, header, checkpoint.CheckpointID, k.domain(params), params.MaxAttestationDelayMs,
		k.EligibleSigner(ctx, params)); err != nil {
		return checkpoint, types.AvailabilityAttestation{}, types.ErrAvailability.Wrap(err.Error())
	}
	record := types.AvailabilityAttestation{BatchNumber: checkpoint.BatchNumber, GuarantorID: attestation.GuarantorID,
		CheckpointID: checkpoint.CheckpointID, ClassMask: attestation.AvailabilityClassMask,
		AttestedAtMs: attestation.AttestedAtMs, Height: ctx.BlockHeight()}
	var previous types.AvailabilityAttestation
	if k.get(ctx, types.AvailabilityKey(record.BatchNumber, record.GuarantorID), &previous) {
		if previous.ClassMask|record.ClassMask == previous.ClassMask {
			return checkpoint, previous, types.ErrAvailability.Wrap("attestation adds no availability class")
		}
		record.ClassMask |= previous.ClassMask
	}
	k.set(ctx, types.AvailabilityKey(record.BatchNumber, record.GuarantorID), record)
	checkpoint.AvailabilityMask = k.availabilityMask(ctx, checkpoint.BatchNumber, params)
	k.setCheckpoint(ctx, checkpoint)
	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventAvailability,
		sdk.NewAttribute(types.AttributeBatchNumber, fmt.Sprint(record.BatchNumber)),
		sdk.NewAttribute(types.AttributeGuarantorID, fmt.Sprintf("%x", record.GuarantorID[:])),
		sdk.NewAttribute(types.AttributeClassMask, fmt.Sprint(record.ClassMask))))
	return checkpoint, record, nil
}
