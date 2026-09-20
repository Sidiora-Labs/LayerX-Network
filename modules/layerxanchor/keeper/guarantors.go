package keeper

import (
	"encoding/json"
	"fmt"

	"github.com/sidiora-labs/paxeer-network/modules/layerxanchor/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

func (k Keeper) GetGuarantor(ctx sdk.Context, id [32]byte) (types.Guarantor, bool) {
	var guarantor types.Guarantor
	ok := k.get(ctx, types.GuarantorKey(id), &guarantor)
	return guarantor, ok
}

func (k Keeper) setGuarantor(ctx sdk.Context, guarantor types.Guarantor) {
	k.set(ctx, types.GuarantorKey(guarantor.ID), guarantor)
}

func (k Keeper) GetGuarantors(ctx sdk.Context) []types.Guarantor {
	var out []types.Guarantor
	k.iterate(ctx, types.GuarantorPrefix, func(value []byte) bool {
		var guarantor types.Guarantor
		if err := json.Unmarshal(value, &guarantor); err != nil {
			panic(err)
		}
		out = append(out, guarantor)
		return false
	})
	return out
}

func (k Keeper) GetUnbondings(ctx sdk.Context) []types.UnbondingEntry {
	var out []types.UnbondingEntry
	k.iterate(ctx, types.UnbondingPrefix, func(value []byte) bool {
		var entry types.UnbondingEntry
		if err := json.Unmarshal(value, &entry); err != nil {
			panic(err)
		}
		out = append(out, entry)
		return false
	})
	return out
}

// eligible is bondedActive: active and bonded to at least the minimum.
func eligible(guarantor types.Guarantor, params types.Params) bool {
	return guarantor.Status == types.GuarantorActive && guarantor.Bond.GTE(params.MinBond)
}

// EligibleSigner resolves the attestation signer of a bonded active guarantor.
func (k Keeper) EligibleSigner(ctx sdk.Context, params types.Params) func([32]byte, uint64) ([20]byte, bool) {
	return func(id [32]byte, _ uint64) ([20]byte, bool) {
		guarantor, ok := k.GetGuarantor(ctx, id)
		if !ok || !eligible(guarantor, params) {
			return [20]byte{}, false
		}
		return guarantor.Signer, true
	}
}

func (k Keeper) coins(ctx sdk.Context, amount sdk.Int) sdk.Coins {
	return sdk.NewCoins(sdk.NewCoin(k.GetParams(ctx).BondDenom, amount))
}

// RegisterGuarantor bonds amount from operator into the module account. The
// guarantor joins the quorum at once when registration is permissionless or
// the operator is the authority; otherwise it waits for ActivateGuarantor.
func (k Keeper) RegisterGuarantor(ctx sdk.Context, operator sdk.AccAddress, id [32]byte, signer [20]byte, amount sdk.Int) (types.Guarantor, error) {
	params := k.GetParams(ctx)
	if id == ([32]byte{}) || signer == ([20]byte{}) || operator.Empty() {
		return types.Guarantor{}, types.ErrBond.Wrap("guarantor identifier, signer and operator are required")
	}
	if amount.IsNil() || amount.LT(params.MinBond) {
		return types.Guarantor{}, types.ErrBond.Wrapf("bond %s is below the minimum %s", amount, params.MinBond)
	}
	if _, exists := k.GetGuarantor(ctx, id); exists {
		return types.Guarantor{}, types.ErrGuarantorExists
	}
	for _, other := range k.GetGuarantors(ctx) {
		if other.Signer == types.Address20(signer) {
			return types.Guarantor{}, types.ErrGuarantorExists.Wrap("signer already registered")
		}
	}
	if err := k.bankKeeper.SendCoinsFromAccountToModule(ctx, operator, types.ModuleName, k.coins(ctx, amount)); err != nil {
		return types.Guarantor{}, err
	}
	status := types.GuarantorPending
	if params.PermissionlessRegistration || params.Authority == operator.String() {
		status = types.GuarantorActive
	}
	guarantor := types.Guarantor{ID: id, Signer: signer, Operator: operator.String(), Bond: amount, Status: status,
		RegisteredHeight: ctx.BlockHeight()}
	k.setGuarantor(ctx, guarantor)
	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventGuarantorRegistered,
		sdk.NewAttribute(types.AttributeGuarantorID, fmt.Sprintf("%x", id[:])),
		sdk.NewAttribute(types.AttributeSigner, fmt.Sprintf("%x", signer[:])),
		sdk.NewAttribute(types.AttributeOperator, operator.String()),
		sdk.NewAttribute(types.AttributeAmount, amount.String())))
	return guarantor, nil
}

// ActivateGuarantor admits a pending guarantor; only the authority may.
func (k Keeper) ActivateGuarantor(ctx sdk.Context, authority sdk.AccAddress, id [32]byte) error {
	if err := k.requireAuthority(ctx, authority); err != nil {
		return err
	}
	guarantor, ok := k.GetGuarantor(ctx, id)
	if !ok {
		return types.ErrGuarantorUnknown
	}
	if guarantor.Status != types.GuarantorPending {
		return types.ErrBond.Wrap("guarantor is not pending")
	}
	guarantor.Status = types.GuarantorActive
	k.setGuarantor(ctx, guarantor)
	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventGuarantorActivated,
		sdk.NewAttribute(types.AttributeGuarantorID, fmt.Sprintf("%x", id[:]))))
	return nil
}

func (k Keeper) operated(ctx sdk.Context, operator sdk.AccAddress, id [32]byte) (types.Guarantor, error) {
	guarantor, ok := k.GetGuarantor(ctx, id)
	if !ok {
		return guarantor, types.ErrGuarantorUnknown
	}
	if guarantor.Operator != operator.String() {
		return guarantor, types.ErrUnauthorized.Wrap("only the operator controls the bond")
	}
	return guarantor, nil
}

func (k Keeper) IncreaseBond(ctx sdk.Context, operator sdk.AccAddress, id [32]byte, amount sdk.Int) (types.Guarantor, error) {
	guarantor, err := k.operated(ctx, operator, id)
	if err != nil {
		return guarantor, err
	}
	if amount.IsNil() || !amount.IsPositive() || guarantor.Status == types.GuarantorEjected {
		return guarantor, types.ErrBond.Wrap("bond increase refused")
	}
	if err := k.bankKeeper.SendCoinsFromAccountToModule(ctx, operator, types.ModuleName, k.coins(ctx, amount)); err != nil {
		return guarantor, err
	}
	guarantor.Bond = guarantor.Bond.Add(amount)
	k.setGuarantor(ctx, guarantor)
	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventBondIncreased,
		sdk.NewAttribute(types.AttributeGuarantorID, fmt.Sprintf("%x", id[:])),
		sdk.NewAttribute(types.AttributeAmount, amount.String())))
	return guarantor, nil
}

// BeginUnbond moves amount out of the bond into the unbonding queue. The
// amount leaves the quorum weight at once and stays slashable until the delay
// elapses.
func (k Keeper) BeginUnbond(ctx sdk.Context, operator sdk.AccAddress, id [32]byte, amount sdk.Int) (types.UnbondingEntry, error) {
	guarantor, err := k.operated(ctx, operator, id)
	if err != nil {
		return types.UnbondingEntry{}, err
	}
	if amount.IsNil() || !amount.IsPositive() || amount.GT(guarantor.Bond) {
		return types.UnbondingEntry{}, types.ErrBond.Wrap("unbond amount outside the bond")
	}
	params := k.GetParams(ctx)
	guarantor.Bond = guarantor.Bond.Sub(amount)
	k.setGuarantor(ctx, guarantor)
	entry := types.UnbondingEntry{
		ID:             k.nextID(ctx, types.NextUnbondingIDKey),
		GuarantorID:    id,
		Operator:       guarantor.Operator,
		Amount:         amount,
		CompletionTime: ctx.BlockTime().Unix() + int64(params.UnbondingDelaySeconds), //nolint:gosec
	}
	k.set(ctx, types.UnbondingKey(entry.ID), entry)
	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventUnbondBegun,
		sdk.NewAttribute(types.AttributeGuarantorID, fmt.Sprintf("%x", id[:])),
		sdk.NewAttribute(types.AttributeAmount, amount.String()),
		sdk.NewAttribute(types.AttributeCompletion, fmt.Sprint(entry.CompletionTime))))
	return entry, nil
}

// CompleteUnbond pays out every matured unbonding entry of the guarantor.
func (k Keeper) CompleteUnbond(ctx sdk.Context, operator sdk.AccAddress, id [32]byte) (sdk.Int, error) {
	if _, err := k.operated(ctx, operator, id); err != nil {
		return sdk.ZeroInt(), err
	}
	now := ctx.BlockTime().Unix()
	total := sdk.ZeroInt()
	pending := false
	for _, entry := range k.GetUnbondings(ctx) {
		if entry.GuarantorID != types.Hash32(id) {
			continue
		}
		if entry.CompletionTime > now {
			pending = true
			continue
		}
		total = total.Add(entry.Amount)
		ctx.KVStore(k.storeKey).Delete(types.UnbondingKey(entry.ID))
	}
	if total.IsZero() {
		if pending {
			return total, types.ErrUnbondingImmature
		}
		return total, types.ErrBond.Wrap("nothing is unbonding")
	}
	if err := k.bankKeeper.SendCoinsFromModuleToAccount(ctx, types.ModuleName, operator, k.coins(ctx, total)); err != nil {
		return sdk.ZeroInt(), err
	}
	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventUnbondCompleted,
		sdk.NewAttribute(types.AttributeGuarantorID, fmt.Sprintf("%x", id[:])),
		sdk.NewAttribute(types.AttributeAmount, total.String())))
	return total, nil
}
