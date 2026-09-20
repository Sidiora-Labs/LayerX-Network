// Package keeper holds the LayerX anchor state: sequencer authorizations, the
// bonded guarantor set that is the finality authority, checkpoints by batch
// number, availability attestations, challenges and slash records. It only
// verifies submitted evidence; nothing here queries LayerX.
package keeper

import (
	"encoding/json"
	"fmt"

	"github.com/sidiora-labs/paxeer-network/layerxproof/verify"
	"github.com/sidiora-labs/paxeer-network/modules/layerxanchor/types"
	"github.com/sidiora-labs/paxeer-network/sdk/store/prefix"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

type Keeper struct {
	storeKey      sdk.StoreKey
	accountKeeper types.AccountKeeper
	bankKeeper    types.BankKeeper
}

func NewKeeper(storeKey sdk.StoreKey, accountKeeper types.AccountKeeper, bankKeeper types.BankKeeper) Keeper {
	if accountKeeper.GetModuleAddress(types.ModuleName) == nil {
		panic(fmt.Sprintf("%s module account has not been set", types.ModuleName))
	}
	return Keeper{storeKey: storeKey, accountKeeper: accountKeeper, bankKeeper: bankKeeper}
}

func (k Keeper) ModuleAddress() sdk.AccAddress {
	return k.accountKeeper.GetModuleAddress(types.ModuleName)
}

func (k Keeper) set(ctx sdk.Context, key []byte, value interface{}) {
	encoded, err := json.Marshal(value)
	if err != nil {
		panic(err)
	}
	ctx.KVStore(k.storeKey).Set(key, encoded)
}

func (k Keeper) get(ctx sdk.Context, key []byte, value interface{}) bool {
	encoded := ctx.KVStore(k.storeKey).Get(key)
	if encoded == nil {
		return false
	}
	if err := json.Unmarshal(encoded, value); err != nil {
		panic(err)
	}
	return true
}

func (k Keeper) iterate(ctx sdk.Context, keyPrefix []byte, visit func(value []byte) (stop bool)) {
	iterator := prefix.NewStore(ctx.KVStore(k.storeKey), keyPrefix).Iterator(nil, nil)
	defer func() { _ = iterator.Close() }()
	for ; iterator.Valid(); iterator.Next() {
		if visit(iterator.Value()) {
			return
		}
	}
}

func (k Keeper) GetParams(ctx sdk.Context) types.Params {
	var params types.Params
	if !k.get(ctx, types.ParamsKey, &params) {
		return types.DefaultParams(types.DefaultAuthority())
	}
	return params
}

func (k Keeper) SetParams(ctx sdk.Context, params types.Params) error {
	if err := params.Validate(); err != nil {
		return err
	}
	k.set(ctx, types.ParamsKey, params)
	return nil
}

func (k Keeper) requireAuthority(ctx sdk.Context, actor sdk.AccAddress) error {
	if actor.Empty() || k.GetParams(ctx).Authority != actor.String() {
		return types.ErrUnauthorized.Wrapf("%s is not the module authority", actor)
	}
	return nil
}

// UpdateParams replaces the params; only the authority may.
func (k Keeper) UpdateParams(ctx sdk.Context, authority sdk.AccAddress, params types.Params) error {
	if err := k.requireAuthority(ctx, authority); err != nil {
		return err
	}
	if err := k.SetParams(ctx, params); err != nil {
		return err
	}
	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventParamsUpdated))
	return nil
}

func (k Keeper) GetAnchor(ctx sdk.Context) types.Anchor {
	var anchor types.Anchor
	k.get(ctx, types.AnchorKey, &anchor)
	return anchor
}

func (k Keeper) nextID(ctx sdk.Context, key []byte) uint64 {
	var next uint64
	if !k.get(ctx, key, &next) {
		next = 1
	}
	k.set(ctx, key, next+1)
	return next
}

func (k Keeper) peekID(ctx sdk.Context, key []byte) uint64 {
	var next uint64
	if !k.get(ctx, key, &next) {
		return 1
	}
	return next
}

// Sequencer authorizations.

func (k Keeper) GetSequencerAuthorizations(ctx sdk.Context) []types.SequencerAuthorization {
	var out []types.SequencerAuthorization
	k.iterate(ctx, types.SequencerPrefix, func(value []byte) bool {
		var authorization types.SequencerAuthorization
		if err := json.Unmarshal(value, &authorization); err != nil {
			panic(err)
		}
		out = append(out, authorization)
		return false
	})
	return out
}

func (k Keeper) setSequencerAuthorization(ctx sdk.Context, authorization types.SequencerAuthorization) {
	k.set(ctx, types.SequencerKey(authorization.SequencerID, authorization.FirstBatchNumber), authorization)
}

// SetSequencerAuthorization adds one authorization; only the authority may,
// and ranges of one sequencer never overlap.
func (k Keeper) SetSequencerAuthorization(ctx sdk.Context, authority sdk.AccAddress, authorization types.SequencerAuthorization) error {
	if err := k.requireAuthority(ctx, authority); err != nil {
		return err
	}
	if !verify.PublicKeyIsCanonical(authorization.PublicKey) {
		return types.ErrSequencerUnauthorized.Wrap("non-canonical sequencer key")
	}
	if err := types.ValidateSequencers(append(k.GetSequencerAuthorizations(ctx), authorization)); err != nil {
		return err
	}
	k.setSequencerAuthorization(ctx, authorization)
	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventSequencerAuthorized,
		sdk.NewAttribute(types.AttributeSequencerID, fmt.Sprintf("%x", authorization.SequencerID[:])),
		sdk.NewAttribute(types.AttributeFirstBatch, fmt.Sprint(authorization.FirstBatchNumber)),
		sdk.NewAttribute(types.AttributeLastBatch, fmt.Sprint(authorization.LastBatchNumber))))
	return nil
}

// SequencerAuthorization returns the authorization of sequencerID covering
// batchNumber.
func (k Keeper) SequencerAuthorization(ctx sdk.Context, sequencerID [32]byte, batchNumber uint64) (verify.SequencerAuthorization, bool) {
	var found *types.SequencerAuthorization
	k.iterate(ctx, types.SequencerIDPrefix(sequencerID), func(value []byte) bool {
		var authorization types.SequencerAuthorization
		if err := json.Unmarshal(value, &authorization); err != nil {
			panic(err)
		}
		if batchNumber >= authorization.FirstBatchNumber && batchNumber <= authorization.LastBatchNumber {
			found = &authorization
			return true
		}
		return false
	})
	if found == nil {
		return verify.SequencerAuthorization{}, false
	}
	return verify.SequencerAuthorization{
		SequencerID:      found.SequencerID,
		PublicKey:        found.PublicKey,
		FirstBatchNumber: found.FirstBatchNumber,
		LastBatchNumber:  found.LastBatchNumber,
	}, true
}

// AnchorReader is the read API other modules consume.
type AnchorReader interface {
	FinalizedStateRoot(ctx sdk.Context, batchNumber uint64) ([32]byte, bool)
	FinalizedReceiptRoot(ctx sdk.Context, batchNumber uint64) ([32]byte, bool)
	LatestFinalizedBatch(ctx sdk.Context) (uint64, bool)
	SequencerAuthorization(ctx sdk.Context, sequencerID [32]byte, batchNumber uint64) (verify.SequencerAuthorization, bool)
}

var _ AnchorReader = Keeper{}

func (k Keeper) FinalizedStateRoot(ctx sdk.Context, batchNumber uint64) ([32]byte, bool) {
	checkpoint, ok := k.GetCheckpoint(ctx, batchNumber)
	if !ok || checkpoint.Status != types.CheckpointFinal {
		return [32]byte{}, false
	}
	return checkpoint.StateRoot, true
}

func (k Keeper) FinalizedReceiptRoot(ctx sdk.Context, batchNumber uint64) ([32]byte, bool) {
	checkpoint, ok := k.GetCheckpoint(ctx, batchNumber)
	if !ok || checkpoint.Status != types.CheckpointFinal {
		return [32]byte{}, false
	}
	return checkpoint.ReceiptRoot, true
}

func (k Keeper) LatestFinalizedBatch(ctx sdk.Context) (uint64, bool) {
	var latest uint64
	if !k.get(ctx, types.LatestFinalizedKey, &latest) {
		return 0, false
	}
	return latest, true
}

// StatusOf is the status ladder: 0 unknown, 1 submitted, 2 final.
func (k Keeper) StatusOf(ctx sdk.Context, batchNumber uint64) uint8 {
	checkpoint, ok := k.GetCheckpoint(ctx, batchNumber)
	if !ok {
		return types.CheckpointUnknown
	}
	return checkpoint.Status
}
