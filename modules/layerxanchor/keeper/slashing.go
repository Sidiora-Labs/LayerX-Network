package keeper

import (
	"encoding/json"
	"fmt"

	"github.com/sidiora-labs/paxeer-network/layerxproof/codec"
	"github.com/sidiora-labs/paxeer-network/layerxproof/verify"
	"github.com/sidiora-labs/paxeer-network/modules/layerxanchor/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

func (k Keeper) GetSlashRecords(ctx sdk.Context) []types.SlashRecord {
	var out []types.SlashRecord
	k.iterate(ctx, types.SlashRecordPrefix, func(value []byte) bool {
		var record types.SlashRecord
		if err := json.Unmarshal(value, &record); err != nil {
			panic(err)
		}
		out = append(out, record)
		return false
	})
	return out
}

// dispose removes forfeited funds from the module account: to the configured
// destination when one is set, burned otherwise.
func (k Keeper) dispose(ctx sdk.Context, params types.Params, amount sdk.Int) error {
	if !amount.IsPositive() {
		return nil
	}
	coins := sdk.NewCoins(sdk.NewCoin(params.BondDenom, amount))
	if params.SlashDestination != "" {
		destination, err := sdk.AccAddressFromBech32(params.SlashDestination)
		if err != nil {
			return err
		}
		return k.bankKeeper.SendCoinsFromModuleToAccount(ctx, types.ModuleName, destination, coins)
	}
	return k.bankKeeper.BurnCoins(ctx, types.ModuleName, coins)
}

// slash takes fraction of the guarantor's bond and of every unbonding entry
// still inside its delay, ejects the guarantor, pays the reporter its share and
// disposes of the rest. One (guarantor, reason, batch) offence is slashed once.
func (k Keeper) slash(ctx sdk.Context, params types.Params, guarantor types.Guarantor, reason uint8, batchNumber uint64,
	fraction sdk.Dec, reporter sdk.AccAddress, first, second [32]byte) (types.SlashRecord, error) {
	key := types.SlashRecordKey(guarantor.ID, reason, batchNumber)
	if ctx.KVStore(k.storeKey).Has(key) {
		return types.SlashRecord{}, types.ErrAlreadySlashed
	}
	total := fraction.MulInt(guarantor.Bond).TruncateInt()
	guarantor.Bond = guarantor.Bond.Sub(total)
	guarantor.Status = types.GuarantorEjected
	k.setGuarantor(ctx, guarantor)
	now := ctx.BlockTime().Unix()
	for _, entry := range k.GetUnbondings(ctx) {
		if entry.GuarantorID != guarantor.ID || entry.CompletionTime <= now {
			continue
		}
		cut := fraction.MulInt(entry.Amount).TruncateInt()
		if !cut.IsPositive() {
			continue
		}
		total = total.Add(cut)
		entry.Amount = entry.Amount.Sub(cut)
		if entry.Amount.IsZero() {
			ctx.KVStore(k.storeKey).Delete(types.UnbondingKey(entry.ID))
		} else {
			k.set(ctx, types.UnbondingKey(entry.ID), entry)
		}
	}
	reward := sdk.ZeroInt()
	if !reporter.Empty() {
		reward = params.ReporterShare.MulInt(total).TruncateInt()
	}
	if reward.IsPositive() {
		if err := k.bankKeeper.SendCoinsFromModuleToAccount(ctx, types.ModuleName, reporter,
			sdk.NewCoins(sdk.NewCoin(params.BondDenom, reward))); err != nil {
			return types.SlashRecord{}, err
		}
	}
	if err := k.dispose(ctx, params, total.Sub(reward)); err != nil {
		return types.SlashRecord{}, err
	}
	record := types.SlashRecord{GuarantorID: guarantor.ID, Reason: reason, BatchNumber: batchNumber, Amount: total,
		ReporterReward: reward, Reporter: reporter.String(), Height: ctx.BlockHeight(), FirstCheckpoint: first, SecondCheckpoint: second}
	k.set(ctx, key, record)
	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventGuarantorSlashed,
		sdk.NewAttribute(types.AttributeGuarantorID, fmt.Sprintf("%x", guarantor.ID[:])),
		sdk.NewAttribute(types.AttributeReason, fmt.Sprint(reason)),
		sdk.NewAttribute(types.AttributeBatchNumber, fmt.Sprint(batchNumber)),
		sdk.NewAttribute(types.AttributeAmount, total.String()),
		sdk.NewAttribute(types.AttributeReporter, reporter.String()),
		sdk.NewAttribute(types.AttributeReward, reward.String())))
	return record, nil
}

// SubmitEquivocation slashes a guarantor on two self-verifying attestations
// that name different checkpoints for one batch. No arbiter is involved:
// anyone may report, and the two signatures are the whole proof. A guarantor
// already ejected by a slash is not slashed again, as GuarantorBond refuses a
// second equivocation after ejection.
func (k Keeper) SubmitEquivocation(ctx sdk.Context, reporter sdk.AccAddress, evidenceA, evidenceB []byte) (types.SlashRecord, error) {
	params := k.GetParams(ctx)
	first, err := codec.DecodeGuarantorAttestation(evidenceA)
	if err != nil {
		return types.SlashRecord{}, types.ErrEvidence.Wrap(err.Error())
	}
	second, err := codec.DecodeGuarantorAttestation(evidenceB)
	if err != nil {
		return types.SlashRecord{}, types.ErrEvidence.Wrap(err.Error())
	}
	if err := verify.GuarantorEquivocation(first, second, k.domain(params)); err != nil {
		return types.SlashRecord{}, types.ErrEvidence.Wrap(err.Error())
	}
	if params.NetworkID != 0 && first.NetworkID != params.NetworkID {
		return types.SlashRecord{}, types.ErrEvidence.Wrap("network identifier")
	}
	guarantor, ok := k.GetGuarantor(ctx, first.GuarantorID)
	if !ok {
		return types.SlashRecord{}, types.ErrGuarantorUnknown
	}
	if guarantor.Signer != types.Address20(first.Signer) {
		return types.SlashRecord{}, types.ErrEvidence.Wrap("statements are not signed by the guarantor's signer")
	}
	if guarantor.Status == types.GuarantorEjected {
		return types.SlashRecord{}, types.ErrAlreadySlashed
	}
	return k.slash(ctx, params, guarantor, types.SlashEquivocation, first.BatchNumber, params.SlashFractionEquivocation,
		reporter, first.CheckpointHash, second.CheckpointHash)
}
