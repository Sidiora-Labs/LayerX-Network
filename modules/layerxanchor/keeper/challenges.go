package keeper

import (
	"encoding/json"
	"errors"
	"fmt"

	"github.com/sidiora-labs/paxeer-network/modules/layerxanchor/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

func (k Keeper) GetChallenge(ctx sdk.Context, id uint64) (types.Challenge, bool) {
	var challenge types.Challenge
	ok := k.get(ctx, types.ChallengeKey(id), &challenge)
	return challenge, ok
}

func (k Keeper) GetChallenges(ctx sdk.Context) []types.Challenge {
	var out []types.Challenge
	k.iterate(ctx, types.ChallengePrefix, func(value []byte) bool {
		var challenge types.Challenge
		if err := json.Unmarshal(value, &challenge); err != nil {
			panic(err)
		}
		out = append(out, challenge)
		return false
	})
	return out
}

// OpenChallenge escrows exactly the challenge bond against a known checkpoint
// and blocks its finalization until the authority resolves the challenge.
func (k Keeper) OpenChallenge(ctx sdk.Context, challenger sdk.AccAddress, batchNumber uint64, kind uint8,
	evidenceHash [32]byte, bond sdk.Int) (types.Challenge, error) {
	params := k.GetParams(ctx)
	if kind > types.ChallengeDataAvailability || evidenceHash == ([32]byte{}) || challenger.Empty() {
		return types.Challenge{}, types.ErrChallenge.Wrap("kind, evidence hash and challenger are required")
	}
	if bond.IsNil() || !bond.Equal(params.ChallengeBond) {
		return types.Challenge{}, types.ErrChallenge.Wrapf("challenge bond must be exactly %s", params.ChallengeBond)
	}
	checkpoint, ok := k.GetCheckpoint(ctx, batchNumber)
	if !ok {
		return types.Challenge{}, types.ErrCheckpointUnknown
	}
	if bond.IsPositive() {
		if err := k.bankKeeper.SendCoinsFromAccountToModule(ctx, challenger, types.ModuleName,
			sdk.NewCoins(sdk.NewCoin(params.BondDenom, bond))); err != nil {
			return types.Challenge{}, err
		}
	}
	challenge := types.Challenge{ID: k.nextID(ctx, types.NextChallengeIDKey), BatchNumber: batchNumber,
		CheckpointID: checkpoint.CheckpointID, Kind: kind, EvidenceHash: evidenceHash, Challenger: challenger.String(),
		Bond: bond, Status: types.ChallengeOpen, OpenedHeight: ctx.BlockHeight(), OpenedTime: ctx.BlockTime().Unix()}
	k.set(ctx, types.ChallengeKey(challenge.ID), challenge)
	checkpoint.OpenChallenges++
	k.setCheckpoint(ctx, checkpoint)
	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventChallengeOpened,
		sdk.NewAttribute(types.AttributeChallengeID, fmt.Sprint(challenge.ID)),
		sdk.NewAttribute(types.AttributeBatchNumber, fmt.Sprint(batchNumber)),
		sdk.NewAttribute(types.AttributeKind, fmt.Sprint(kind))))
	return challenge, nil
}

// ResolveChallenge is the arbiter's decision and only the authority may make
// it. Upheld: the challenger's bond is returned, every guarantor that attested
// the checkpoint is slashed with the challenger as reporter, and a checkpoint
// that is not yet final is removed. Rejected: the challenger's bond is
// forfeited. A finalized checkpoint is never rewritten.
func (k Keeper) ResolveChallenge(ctx sdk.Context, authority sdk.AccAddress, id uint64, upheld bool) (types.Challenge, []types.SlashRecord, error) {
	if err := k.requireAuthority(ctx, authority); err != nil {
		return types.Challenge{}, nil, err
	}
	params := k.GetParams(ctx)
	var slashed []types.SlashRecord
	challenge, ok := k.GetChallenge(ctx, id)
	if !ok || challenge.Status != types.ChallengeOpen {
		return challenge, nil, types.ErrChallenge.Wrap("challenge is not open")
	}
	challenger, err := sdk.AccAddressFromBech32(challenge.Challenger)
	if err != nil {
		return challenge, nil, err
	}
	checkpoint, present := k.GetCheckpoint(ctx, challenge.BatchNumber)
	present = present && checkpoint.CheckpointID == challenge.CheckpointID
	if present && checkpoint.OpenChallenges > 0 {
		checkpoint.OpenChallenges--
		k.setCheckpoint(ctx, checkpoint)
	}
	if upheld {
		challenge.Status = types.ChallengeUpheld
		if challenge.Bond.IsPositive() {
			if err := k.bankKeeper.SendCoinsFromModuleToAccount(ctx, types.ModuleName, challenger,
				sdk.NewCoins(sdk.NewCoin(params.BondDenom, challenge.Bond))); err != nil {
				return challenge, nil, err
			}
		}
		if present {
			reason, fraction := types.SlashFraud, params.SlashFractionFraud
			if challenge.Kind == types.ChallengeDataAvailability {
				reason, fraction = types.SlashDataAvailability, params.SlashFractionAvailability
			}
			for _, guarantorID := range checkpoint.Guarantors {
				guarantor, known := k.GetGuarantor(ctx, guarantorID)
				if !known {
					continue
				}
				record, err := k.slash(ctx, params, guarantor, reason, checkpoint.BatchNumber, fraction, challenger,
					checkpoint.CheckpointID, [32]byte{})
				if errors.Is(err, types.ErrAlreadySlashed) {
					continue
				}
				if err != nil {
					return challenge, nil, err
				}
				slashed = append(slashed, record)
			}
			if checkpoint.Status != types.CheckpointFinal {
				k.deleteCheckpoint(ctx, checkpoint)
				k.clearAvailability(ctx, checkpoint.BatchNumber)
			}
		}
	} else {
		challenge.Status = types.ChallengeRejected
		if err := k.dispose(ctx, params, challenge.Bond); err != nil {
			return challenge, nil, err
		}
	}
	challenge.ResolvedHeight = ctx.BlockHeight()
	k.set(ctx, types.ChallengeKey(challenge.ID), challenge)
	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventChallengeResolved,
		sdk.NewAttribute(types.AttributeChallengeID, fmt.Sprint(challenge.ID)),
		sdk.NewAttribute(types.AttributeBatchNumber, fmt.Sprint(challenge.BatchNumber)),
		sdk.NewAttribute(types.AttributeUpheld, fmt.Sprint(upheld))))
	return challenge, slashed, nil
}
