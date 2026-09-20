package keeper

import (
	"fmt"

	"github.com/sidiora-labs/paxeer-network/modules/layerxanchor/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// Escrowed is what the module account must hold: every bond, every unbonding
// entry and every open challenge bond.
func (k Keeper) Escrowed(ctx sdk.Context) sdk.Int {
	total := sdk.ZeroInt()
	for _, guarantor := range k.GetGuarantors(ctx) {
		total = total.Add(guarantor.Bond)
	}
	for _, entry := range k.GetUnbondings(ctx) {
		total = total.Add(entry.Amount)
	}
	for _, challenge := range k.GetChallenges(ctx) {
		if challenge.Status == types.ChallengeOpen {
			total = total.Add(challenge.Bond)
		}
	}
	return total
}

func (k Keeper) ModuleBalance(ctx sdk.Context) sdk.Int {
	return k.bankKeeper.GetBalance(ctx, k.ModuleAddress(), k.GetParams(ctx).BondDenom).Amount
}

func (k Keeper) InitGenesis(ctx sdk.Context, genesis types.GenesisState) {
	if err := genesis.Validate(); err != nil {
		panic(err)
	}
	k.accountKeeper.GetModuleAccount(ctx, types.ModuleName)
	if err := k.SetParams(ctx, genesis.Params); err != nil {
		panic(err)
	}
	if genesis.Anchor.Set {
		k.set(ctx, types.AnchorKey, genesis.Anchor)
	}
	for _, authorization := range genesis.Sequencers {
		k.setSequencerAuthorization(ctx, authorization)
	}
	for _, guarantor := range genesis.Guarantors {
		k.setGuarantor(ctx, guarantor)
	}
	for _, entry := range genesis.Unbondings {
		k.set(ctx, types.UnbondingKey(entry.ID), entry)
	}
	for _, checkpoint := range genesis.Checkpoints {
		k.setCheckpoint(ctx, checkpoint)
	}
	if genesis.HasLatestFinalized {
		k.set(ctx, types.LatestFinalizedKey, genesis.LatestFinalized)
	}
	for _, attestation := range genesis.Availability {
		k.set(ctx, types.AvailabilityKey(attestation.BatchNumber, attestation.GuarantorID), attestation)
	}
	for _, challenge := range genesis.Challenges {
		k.set(ctx, types.ChallengeKey(challenge.ID), challenge)
	}
	for _, record := range genesis.SlashRecords {
		k.set(ctx, types.SlashRecordKey(record.GuarantorID, record.Reason, record.BatchNumber), record)
	}
	k.set(ctx, types.NextChallengeIDKey, genesis.NextChallengeID)
	k.set(ctx, types.NextUnbondingIDKey, genesis.NextUnbondingID)
	if escrowed, balance := k.Escrowed(ctx), k.ModuleBalance(ctx); !escrowed.Equal(balance) {
		panic(fmt.Sprintf("%s module account holds %s but genesis bonds, unbondings and challenge bonds total %s",
			types.ModuleName, balance, escrowed))
	}
}

func (k Keeper) ExportGenesis(ctx sdk.Context) *types.GenesisState {
	genesis := &types.GenesisState{
		Params:          k.GetParams(ctx),
		Anchor:          k.GetAnchor(ctx),
		Sequencers:      k.GetSequencerAuthorizations(ctx),
		Guarantors:      k.GetGuarantors(ctx),
		Unbondings:      k.GetUnbondings(ctx),
		Checkpoints:     k.GetCheckpoints(ctx),
		Availability:    k.GetAvailability(ctx),
		Challenges:      k.GetChallenges(ctx),
		SlashRecords:    k.GetSlashRecords(ctx),
		NextChallengeID: k.peekID(ctx, types.NextChallengeIDKey),
		NextUnbondingID: k.peekID(ctx, types.NextUnbondingIDKey),
	}
	genesis.LatestFinalized, genesis.HasLatestFinalized = k.LatestFinalizedBatch(ctx)
	return genesis
}

// BalanceInvariant: the module account holds exactly the escrowed total.
func BalanceInvariant(k Keeper) sdk.Invariant {
	return func(ctx sdk.Context) (string, bool) {
		escrowed, balance := k.Escrowed(ctx), k.ModuleBalance(ctx)
		broken := !escrowed.Equal(balance)
		return sdk.FormatInvariant(types.ModuleName, "module-balance",
			fmt.Sprintf("module balance %s, bonds + unbondings + open challenge bonds %s", balance, escrowed)), broken
	}
}

func RegisterInvariants(registry sdk.InvariantRegistry, k Keeper) {
	registry.RegisterRoute(types.ModuleName, "module-balance", BalanceInvariant(k))
}
