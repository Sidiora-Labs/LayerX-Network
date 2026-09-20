package keeper

import (
	"github.com/sidiora-labs/paxeer-network/layerxproof/verify"
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
