package types

import (
	"github.com/sidiora-labs/paxeer-network/layerxproof/verify"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// AnchorReader is everything custody trusts about LayerX. A withdrawal or a
// forced exit is accepted only against material this interface returns; no
// caller-supplied key or root is ever trusted.
//
// Until the anchor module (precompile 0x1014) lands, the custody keeper
// implements it from its own authority-set state: Params.SequencerAuthorizations
// and the checkpoints recorded by MsgRegisterCheckpoint or genesis. The anchor
// keeper satisfies the same four methods later and replaces that
// implementation through Keeper.SetAnchorReader without a state migration of
// claims, nullifiers or deposits.
type AnchorReader interface {
	// SequencerAuthorization returns the sequencer authority whose inclusive
	// batch range covers batchNumber.
	SequencerAuthorization(ctx sdk.Context, batchNumber uint64) (verify.SequencerAuthorization, bool)
	// FinalizedStateRoot returns the resulting state root of a finalized batch.
	FinalizedStateRoot(ctx sdk.Context, batchNumber uint64) ([32]byte, bool)
	// FinalizedReceiptRoot returns the receipt Merkle root of a finalized batch.
	FinalizedReceiptRoot(ctx sdk.Context, batchNumber uint64) ([32]byte, bool)
	// LatestFinalizedBatch returns the highest finalized batch number and the
	// unix second it was finalized on Paxeer.
	LatestFinalizedBatch(ctx sdk.Context) (batchNumber uint64, finalizedAt int64, ok bool)
}
