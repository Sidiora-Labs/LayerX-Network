package keeper

import (
	"bytes"

	"github.com/sidiora-labs/paxeer-network/layerxproof/codec"
	"github.com/sidiora-labs/paxeer-network/layerxproof/verify"
	"github.com/sidiora-labs/paxeer-network/modules/layerxexchange/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
)

// ProvenState is one LayerX state entry proven under a finalized state root.
type ProvenState struct {
	BatchNumber uint64
	StateRoot   [32]byte
	ModuleID    uint16
	Key         []byte
	Value       []byte
}

// ProvenMargin is one LayerX account balance proven under a finalized state
// root.
type ProvenMargin struct {
	BatchNumber uint64
	StateRoot   [32]byte
	Account     *codec.Account
}

// finalizedRoot is the only root a view trusts: the state root custody's
// AnchorReader returns for a finalized batch. Nothing is read over the network.
func (k *Keeper) finalizedRoot(ctx sdk.Context, batchNumber uint64, witness []byte) ([32]byte, error) {
	if len(witness) > types.MaxWitnessBytes {
		return [32]byte{}, types.ErrEvidenceTooLong
	}
	root, found := k.custody.Anchor().FinalizedStateRoot(ctx, batchNumber)
	if !found {
		return [32]byte{}, sdkerrors.Wrapf(types.ErrNotFinalized, "batch %d", batchNumber)
	}
	return root, nil
}

// ProveState verifies witness under the finalized state root of batchNumber
// and requires it to prove exactly key in module moduleID.
func (k *Keeper) ProveState(ctx sdk.Context, batchNumber uint64, witness []byte, moduleID uint16, key []byte) (ProvenState, error) {
	root, err := k.finalizedRoot(ctx, batchNumber, witness)
	if err != nil {
		return ProvenState{}, err
	}
	proven, err := verify.StateProof(witness, root)
	if err != nil {
		return ProvenState{}, sdkerrors.Wrap(types.ErrInvalidProof, err.Error())
	}
	if proven.ModuleID != moduleID || !bytes.Equal(proven.Key, key) || len(proven.Value) == 0 {
		return ProvenState{}, types.ErrStateMismatch
	}
	return ProvenState{BatchNumber: batchNumber, StateRoot: root, ModuleID: proven.ModuleID,
		Key: proven.Key, Value: proven.Value}, nil
}

// ProveMarket proves the perps market entry of marketID.
func (k *Keeper) ProveMarket(ctx sdk.Context, marketID [32]byte, batchNumber uint64, witness []byte) (ProvenState, error) {
	return k.ProveState(ctx, batchNumber, witness, types.PerpsModuleID, types.MarketStateKey(marketID))
}

// ProveOrder proves the perps order entry of orderID in marketID.
func (k *Keeper) ProveOrder(ctx sdk.Context, marketID, orderID [32]byte, batchNumber uint64, witness []byte) (ProvenState, error) {
	return k.ProveState(ctx, batchNumber, witness, types.PerpsModuleID, types.OrderStateKey(marketID, orderID))
}

// ProvePosition proves the perps position entry of positionID in marketID.
func (k *Keeper) ProvePosition(ctx sdk.Context, marketID, positionID [32]byte, batchNumber uint64, witness []byte) (ProvenState, error) {
	return k.ProveState(ctx, batchNumber, witness, types.PerpsModuleID, types.PositionStateKey(marketID, positionID))
}

// ProveMargin proves the canonical LayerX account holding margin in assetID.
func (k *Keeper) ProveMargin(ctx sdk.Context, account, assetID [32]byte, batchNumber uint64, witness []byte) (ProvenMargin, error) {
	root, err := k.finalizedRoot(ctx, batchNumber, witness)
	if err != nil {
		return ProvenMargin{}, err
	}
	proven, err := verify.AccountProof(witness, root, account, &assetID)
	if err != nil {
		return ProvenMargin{}, sdkerrors.Wrap(types.ErrInvalidProof, err.Error())
	}
	return ProvenMargin{BatchNumber: batchNumber, StateRoot: root, Account: proven}, nil
}
