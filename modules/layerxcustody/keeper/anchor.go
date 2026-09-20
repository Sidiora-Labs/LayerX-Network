package keeper

import (
	"encoding/binary"

	"github.com/sidiora-labs/paxeer-network/layerxproof/verify"
	"github.com/sidiora-labs/paxeer-network/modules/layerxcustody/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
)

// authorityAnchor implements types.AnchorReader from custody's own
// authority-set state until the anchor module replaces it.
type authorityAnchor struct{ k *Keeper }

var _ types.AnchorReader = authorityAnchor{}

func (a authorityAnchor) SequencerAuthorization(ctx sdk.Context, batchNumber uint64) (verify.SequencerAuthorization, bool) {
	for _, candidate := range a.k.GetParams(ctx).SequencerAuthorizations {
		if batchNumber < candidate.FirstBatchNumber || batchNumber > candidate.LastBatchNumber {
			continue
		}
		decoded, err := candidate.Decode()
		if err != nil {
			return verify.SequencerAuthorization{}, false
		}
		return decoded, true
	}
	return verify.SequencerAuthorization{}, false
}

func (a authorityAnchor) FinalizedStateRoot(ctx sdk.Context, batchNumber uint64) ([32]byte, bool) {
	checkpoint, found := a.k.GetCheckpoint(ctx, batchNumber)
	if !found {
		return [32]byte{}, false
	}
	root, err := types.ParseHash32(checkpoint.StateRoot)
	return root, err == nil
}

func (a authorityAnchor) FinalizedReceiptRoot(ctx sdk.Context, batchNumber uint64) ([32]byte, bool) {
	checkpoint, found := a.k.GetCheckpoint(ctx, batchNumber)
	if !found {
		return [32]byte{}, false
	}
	root, err := types.ParseHash32(checkpoint.ReceiptRoot)
	return root, err == nil
}

func (a authorityAnchor) LatestFinalizedBatch(ctx sdk.Context) (uint64, int64, bool) {
	bz := a.k.store(ctx).Get(types.LatestCheckpoint)
	if bz == nil {
		return 0, 0, false
	}
	checkpoint, found := a.k.GetCheckpoint(ctx, binary.BigEndian.Uint64(bz))
	return checkpoint.BatchNumber, checkpoint.FinalizedAt, found
}

func (k *Keeper) GetCheckpoint(ctx sdk.Context, batchNumber uint64) (types.Checkpoint, bool) {
	var checkpoint types.Checkpoint
	bz := k.store(ctx).Get(types.CheckpointKey(batchNumber))
	if bz == nil {
		return checkpoint, false
	}
	k.cdc.MustUnmarshal(bz, &checkpoint)
	return checkpoint, true
}

func (k *Keeper) setCheckpoint(ctx sdk.Context, checkpoint types.Checkpoint) {
	k.store(ctx).Set(types.CheckpointKey(checkpoint.BatchNumber), k.cdc.MustMarshal(&checkpoint))
	latest := k.store(ctx).Get(types.LatestCheckpoint)
	if latest == nil || binary.BigEndian.Uint64(latest) <= checkpoint.BatchNumber {
		k.store(ctx).Set(types.LatestCheckpoint, binary.BigEndian.AppendUint64(nil, checkpoint.BatchNumber))
	}
}

// RegisterCheckpoint records a finalized LayerX batch. A recorded checkpoint
// is immutable: claims already queued against it must stay verifiable.
func (k *Keeper) RegisterCheckpoint(ctx sdk.Context, batchNumber uint64, stateRoot, receiptRoot [32]byte) error {
	checkpoint := types.Checkpoint{BatchNumber: batchNumber, StateRoot: types.Hash32(stateRoot),
		ReceiptRoot: types.Hash32(receiptRoot), FinalizedAt: ctx.BlockTime().Unix()}
	if err := checkpoint.Validate(); err != nil {
		return err
	}
	if _, found := k.GetCheckpoint(ctx, batchNumber); found {
		return sdkerrors.Wrapf(types.ErrInvalidCheckpoint, "batch %d is already finalized", batchNumber)
	}
	k.setCheckpoint(ctx, checkpoint)
	return ctx.EventManager().EmitTypedEvent(&types.EventCheckpointRegistered{BatchNumber: batchNumber,
		StateRoot: checkpoint.StateRoot, ReceiptRoot: checkpoint.ReceiptRoot})
}

func (k *Keeper) IterateCheckpoints(ctx sdk.Context, visit func(types.Checkpoint) bool) {
	k.iterate(ctx, types.CheckpointPrefix, func(value []byte) bool {
		var checkpoint types.Checkpoint
		k.cdc.MustUnmarshal(value, &checkpoint)
		return visit(checkpoint)
	})
}
