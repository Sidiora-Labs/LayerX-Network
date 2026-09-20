package types

import (
	"bytes"

	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	authtypes "github.com/sidiora-labs/paxeer-network/sdk/x/auth/types"
	govtypes "github.com/sidiora-labs/paxeer-network/sdk/x/gov/types"
)

type GenesisState struct {
	Params             Params                    `json:"params"`
	Anchor             Anchor                    `json:"anchor"`
	Sequencers         []SequencerAuthorization  `json:"sequencers"`
	Guarantors         []Guarantor               `json:"guarantors"`
	Unbondings         []UnbondingEntry          `json:"unbondings"`
	Checkpoints        []Checkpoint              `json:"checkpoints"`
	HasLatestFinalized bool                      `json:"has_latest_finalized"`
	LatestFinalized    uint64                    `json:"latest_finalized"`
	Availability       []AvailabilityAttestation `json:"availability"`
	Challenges         []Challenge               `json:"challenges"`
	SlashRecords       []SlashRecord             `json:"slash_records"`
	NextChallengeID    uint64                    `json:"next_challenge_id"`
	NextUnbondingID    uint64                    `json:"next_unbonding_id"`
}

// DefaultAuthority is the gov module account.
func DefaultAuthority() string {
	return authtypes.NewModuleAddress(govtypes.ModuleName).String()
}

func DefaultGenesis() *GenesisState {
	return &GenesisState{Params: DefaultParams(DefaultAuthority()), NextChallengeID: 1, NextUnbondingID: 1}
}

// ValidateSequencers refuses reversed ranges, non-canonical keys left to the
// verifier, and overlapping ranges of one sequencer.
func ValidateSequencers(list []SequencerAuthorization) error {
	for i, a := range list {
		if a.SequencerID == (Hash32{}) || a.PublicKey == (Hash32{}) || a.FirstBatchNumber > a.LastBatchNumber {
			return ErrInvalidGenesis.Wrap("sequencer authorization")
		}
		for _, b := range list[:i] {
			if a.SequencerID == b.SequencerID && a.FirstBatchNumber <= b.LastBatchNumber && b.FirstBatchNumber <= a.LastBatchNumber {
				return ErrInvalidGenesis.Wrap("overlapping sequencer authorizations")
			}
		}
	}
	return nil
}

func (g GenesisState) Validate() error {
	if err := g.Params.Validate(); err != nil {
		return err
	}
	if err := ValidateSequencers(g.Sequencers); err != nil {
		return err
	}
	seen := map[Hash32]bool{}
	signers := map[Address20]bool{}
	for _, guarantor := range g.Guarantors {
		if guarantor.ID == (Hash32{}) || guarantor.Signer == (Address20{}) || seen[guarantor.ID] || signers[guarantor.Signer] ||
			guarantor.Bond.IsNil() || guarantor.Bond.IsNegative() ||
			guarantor.Status < GuarantorPending || guarantor.Status > GuarantorEjected {
			return ErrInvalidGenesis.Wrap("guarantor")
		}
		if _, err := sdk.AccAddressFromBech32(guarantor.Operator); err != nil {
			return ErrInvalidGenesis.Wrapf("guarantor operator: %s", err)
		}
		seen[guarantor.ID] = true
		signers[guarantor.Signer] = true
	}
	for _, entry := range g.Unbondings {
		if !seen[entry.GuarantorID] || entry.Amount.IsNil() || !entry.Amount.IsPositive() || entry.ID == 0 || entry.ID >= g.NextUnbondingID {
			return ErrInvalidGenesis.Wrap("unbonding entry")
		}
	}
	var previous *Checkpoint
	batches := map[uint64]Checkpoint{}
	for i := range g.Checkpoints {
		checkpoint := g.Checkpoints[i]
		if checkpoint.BatchNumber == 0 || checkpoint.Status < CheckpointSubmitted || checkpoint.Status > CheckpointFinal {
			return ErrInvalidGenesis.Wrap("checkpoint")
		}
		if previous != nil && previous.BatchNumber >= checkpoint.BatchNumber {
			return ErrInvalidGenesis.Wrap("checkpoints must ascend by batch number")
		}
		for j := 1; j < len(checkpoint.Guarantors); j++ {
			if bytes.Compare(checkpoint.Guarantors[j-1][:], checkpoint.Guarantors[j][:]) >= 0 {
				return ErrInvalidGenesis.Wrap("checkpoint guarantors must ascend")
			}
		}
		batches[checkpoint.BatchNumber] = checkpoint
		previous = &g.Checkpoints[i]
	}
	if g.HasLatestFinalized {
		if latest, ok := batches[g.LatestFinalized]; !ok || latest.Status != CheckpointFinal {
			return ErrInvalidGenesis.Wrap("latest finalized pointer")
		}
	}
	for _, attestation := range g.Availability {
		if _, ok := batches[attestation.BatchNumber]; !ok || attestation.ClassMask == 0 || attestation.ClassMask > 0x1f {
			return ErrInvalidGenesis.Wrap("availability attestation")
		}
	}
	for _, challenge := range g.Challenges {
		if challenge.ID == 0 || challenge.ID >= g.NextChallengeID || challenge.Kind > ChallengeDataAvailability ||
			challenge.Status < ChallengeOpen || challenge.Status > ChallengeRejected || challenge.Bond.IsNil() || challenge.Bond.IsNegative() {
			return ErrInvalidGenesis.Wrap("challenge")
		}
	}
	if g.NextChallengeID == 0 || g.NextUnbondingID == 0 {
		return ErrInvalidGenesis.Wrap("identifier counters start at one")
	}
	return nil
}
