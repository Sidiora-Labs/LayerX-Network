package types

import (
	"encoding/hex"
	"encoding/json"
	"fmt"

	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// Hash32 is a 32-byte value rendered as hex in genesis and state JSON.
type Hash32 [32]byte

func (h Hash32) MarshalJSON() ([]byte, error) { return json.Marshal(hex.EncodeToString(h[:])) }

func (h *Hash32) UnmarshalJSON(raw []byte) error { return unmarshalHex(raw, h[:]) }

// Address20 is a 20-byte EVM address rendered as hex.
type Address20 [20]byte

func (a Address20) MarshalJSON() ([]byte, error) { return json.Marshal(hex.EncodeToString(a[:])) }

func (a *Address20) UnmarshalJSON(raw []byte) error { return unmarshalHex(raw, a[:]) }

func unmarshalHex(raw []byte, out []byte) error {
	var text string
	if err := json.Unmarshal(raw, &text); err != nil {
		return err
	}
	decoded, err := hex.DecodeString(text)
	if err != nil {
		return err
	}
	if len(decoded) != len(out) {
		return fmt.Errorf("expected %d bytes, got %d", len(out), len(decoded))
	}
	copy(out, decoded)
	return nil
}

// SequencerAuthorization mirrors layerxproof/verify.SequencerAuthorization.
type SequencerAuthorization struct {
	SequencerID      Hash32 `json:"sequencer_id"`
	PublicKey        Hash32 `json:"public_key"`
	FirstBatchNumber uint64 `json:"first_batch_number"`
	LastBatchNumber  uint64 `json:"last_batch_number"`
}

const (
	GuarantorPending uint8 = 1
	GuarantorActive  uint8 = 2
	GuarantorEjected uint8 = 3
)

// Guarantor is one member of the finality authority. Signer is the EVM
// address its attestations recover to; Operator controls the bond.
type Guarantor struct {
	ID               Hash32    `json:"id"`
	Signer           Address20 `json:"signer"`
	Operator         string    `json:"operator"`
	Bond             sdk.Int   `json:"bond"`
	Status           uint8     `json:"status"`
	RegisteredHeight int64     `json:"registered_height"`
}

// UnbondingEntry is bond leaving a guarantor. It stays in the module account
// and stays slashable until CompletionTime.
type UnbondingEntry struct {
	ID             uint64  `json:"id"`
	GuarantorID    Hash32  `json:"guarantor_id"`
	Operator       string  `json:"operator"`
	Amount         sdk.Int `json:"amount"`
	CompletionTime int64   `json:"completion_time"`
}

const (
	CheckpointUnknown   uint8 = 0
	CheckpointSubmitted uint8 = 1
	CheckpointFinal     uint8 = 2
)

// Checkpoint is one LayerX batch checkpoint keyed by batch number.
type Checkpoint struct {
	BatchNumber          uint64   `json:"batch_number"`
	CheckpointID         Hash32   `json:"checkpoint_id"`
	HeaderDigest         Hash32   `json:"header_digest"`
	ProtocolVersion      uint16   `json:"protocol_version"`
	NetworkID            uint32   `json:"network_id"`
	Epoch                uint64   `json:"epoch"`
	FirstSequence        uint64   `json:"first_sequence"`
	LastSequence         uint64   `json:"last_sequence"`
	PreviousStateRoot    Hash32   `json:"previous_state_root"`
	StateRoot            Hash32   `json:"state_root"`
	ReceiptRoot          Hash32   `json:"receipt_root"`
	DataAvailabilityRoot Hash32   `json:"data_availability_root"`
	SequencerID          Hash32   `json:"sequencer_id"`
	TimestampMs          uint64   `json:"timestamp_ms"`
	Status               uint8    `json:"status"`
	DeclaredThreshold    uint8    `json:"declared_threshold"`
	Guarantors           []Hash32 `json:"guarantors"`
	AvailabilityMask     uint8    `json:"availability_mask"`
	Submitter            string   `json:"submitter"`
	SubmittedHeight      int64    `json:"submitted_height"`
	SubmittedTime        int64    `json:"submitted_time"`
	FinalizedHeight      int64    `json:"finalized_height"`
	FinalizedTime        int64    `json:"finalized_time"`
	OpenChallenges       uint32   `json:"open_challenges"`
}

// AvailabilityAttestation records the classes one guarantor attested to
// possess for one checkpoint.
type AvailabilityAttestation struct {
	BatchNumber  uint64 `json:"batch_number"`
	GuarantorID  Hash32 `json:"guarantor_id"`
	CheckpointID Hash32 `json:"checkpoint_id"`
	ClassMask    uint8  `json:"class_mask"`
	AttestedAtMs uint64 `json:"attested_at_ms"`
	Height       int64  `json:"height"`
}

const (
	ChallengeFraud            uint8 = 0
	ChallengeDataAvailability uint8 = 1

	ChallengeOpen     uint8 = 1
	ChallengeUpheld   uint8 = 2
	ChallengeRejected uint8 = 3
)

// Challenge is a governance-arbitrated fraud or data-availability challenge.
// Only the evidence commitment is stored, as CheckpointChallengeManager does.
type Challenge struct {
	ID             uint64  `json:"id"`
	BatchNumber    uint64  `json:"batch_number"`
	CheckpointID   Hash32  `json:"checkpoint_id"`
	Kind           uint8   `json:"kind"`
	EvidenceHash   Hash32  `json:"evidence_hash"`
	Challenger     string  `json:"challenger"`
	Bond           sdk.Int `json:"bond"`
	Status         uint8   `json:"status"`
	OpenedHeight   int64   `json:"opened_height"`
	OpenedTime     int64   `json:"opened_time"`
	ResolvedHeight int64   `json:"resolved_height"`
}

const (
	SlashEquivocation     uint8 = 1
	SlashFraud            uint8 = 2
	SlashDataAvailability uint8 = 3
)

type SlashRecord struct {
	GuarantorID      Hash32  `json:"guarantor_id"`
	Reason           uint8   `json:"reason"`
	BatchNumber      uint64  `json:"batch_number"`
	Amount           sdk.Int `json:"amount"`
	ReporterReward   sdk.Int `json:"reporter_reward"`
	Reporter         string  `json:"reporter"`
	Height           int64   `json:"height"`
	FirstCheckpoint  Hash32  `json:"first_checkpoint"`
	SecondCheckpoint Hash32  `json:"second_checkpoint"`
}

// Anchor is the genesis-settable point the first checkpoint must continue.
type Anchor struct {
	Set          bool   `json:"set"`
	BatchNumber  uint64 `json:"batch_number"`
	LastSequence uint64 `json:"last_sequence"`
	StateRoot    Hash32 `json:"state_root"`
}
