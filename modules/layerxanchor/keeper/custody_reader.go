package keeper

import (
	"github.com/sidiora-labs/paxeer-network/layerxproof/verify"
	"github.com/sidiora-labs/paxeer-network/modules/layerxanchor/types"
	custodytypes "github.com/sidiora-labs/paxeer-network/modules/layerxcustody/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// SequencerAuthorizationForBatch resolves the authorization covering
// batchNumber for a caller that does not yet hold the header's sequencer
// identifier. Ranges of one sequencer never overlap, but two sequencers may
// both be authorised for one batch; that is ambiguous and is refused rather
// than resolved by picking one.
func (k Keeper) SequencerAuthorizationForBatch(ctx sdk.Context, batchNumber uint64) (verify.SequencerAuthorization, bool) {
	var covering []verify.SequencerAuthorization
	for _, authorization := range k.GetSequencerAuthorizations(ctx) {
		if batchNumber < authorization.FirstBatchNumber || batchNumber > authorization.LastBatchNumber {
			continue
		}
		covering = append(covering, verify.SequencerAuthorization{
			SequencerID:      authorization.SequencerID,
			PublicKey:        authorization.PublicKey,
			FirstBatchNumber: authorization.FirstBatchNumber,
			LastBatchNumber:  authorization.LastBatchNumber,
		})
	}
	if len(covering) != 1 {
		return verify.SequencerAuthorization{}, false
	}
	return covering[0], true
}

// CustodyAnchor adapts the anchor keeper to the four-method reader the
// custody module verifies withdrawals and forced exits against
// (modules/layerxcustody/types.AnchorReader). Only finalized checkpoints are
// ever returned.
type CustodyAnchor struct{ k Keeper }

func NewCustodyAnchor(k Keeper) CustodyAnchor { return CustodyAnchor{k: k} }

func (a CustodyAnchor) SequencerAuthorization(ctx sdk.Context, batchNumber uint64) (verify.SequencerAuthorization, bool) {
	return a.k.SequencerAuthorizationForBatch(ctx, batchNumber)
}

func (a CustodyAnchor) FinalizedStateRoot(ctx sdk.Context, batchNumber uint64) ([32]byte, bool) {
	return a.k.FinalizedStateRoot(ctx, batchNumber)
}

func (a CustodyAnchor) FinalizedReceiptRoot(ctx sdk.Context, batchNumber uint64) ([32]byte, bool) {
	return a.k.FinalizedReceiptRoot(ctx, batchNumber)
}

// FinalizedCheckpoint returns the final checkpoint recorded under checkpointID
// with the account that submitted it.
func (a CustodyAnchor) FinalizedCheckpoint(ctx sdk.Context, checkpointID [32]byte) (custodytypes.FinalizedCheckpoint, bool) {
	checkpoint, found := a.k.CheckpointByID(ctx, checkpointID)
	if !found || checkpoint.Status != types.CheckpointFinal {
		return custodytypes.FinalizedCheckpoint{}, false
	}
	proposer, err := sdk.AccAddressFromBech32(checkpoint.Submitter)
	if err != nil {
		return custodytypes.FinalizedCheckpoint{}, false
	}
	return custodytypes.FinalizedCheckpoint{BatchNumber: checkpoint.BatchNumber, StateRoot: checkpoint.StateRoot,
		NetworkID: checkpoint.NetworkID, ProtocolVersion: checkpoint.ProtocolVersion, Proposer: proposer}, true
}

// LatestFinalizedBatch returns the highest finalized batch and the unix second
// it was finalized on Paxeer.
func (a CustodyAnchor) LatestFinalizedBatch(ctx sdk.Context) (uint64, int64, bool) {
	latest, ok := a.k.LatestFinalizedBatch(ctx)
	if !ok {
		return 0, 0, false
	}
	checkpoint, found := a.k.GetCheckpoint(ctx, latest)
	if !found {
		return 0, 0, false
	}
	return checkpoint.BatchNumber, checkpoint.FinalizedTime, true
}
