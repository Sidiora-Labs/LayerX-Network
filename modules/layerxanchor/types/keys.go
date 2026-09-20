package types

import "encoding/binary"

const (
	ModuleName   = "layerxanchor"
	StoreKey     = ModuleName
	RouterKey    = ModuleName
	QuerierRoute = ModuleName
)

var (
	ParamsKey                = []byte{0x01}
	SequencerPrefix          = []byte{0x02}
	GuarantorPrefix          = []byte{0x03}
	UnbondingPrefix          = []byte{0x04}
	CheckpointPrefix         = []byte{0x05}
	LatestFinalizedKey       = []byte{0x06}
	AvailabilityPrefix       = []byte{0x07}
	ChallengePrefix          = []byte{0x08}
	NextChallengeIDKey       = []byte{0x09}
	SlashRecordPrefix        = []byte{0x0a}
	NextUnbondingIDKey       = []byte{0x0b}
	AnchorKey                = []byte{0x0c}
	CheckpointAttesterPrefix = []byte{0x0d}
	CheckpointIDPrefix       = []byte{0x0e}
)

func u64(value uint64) []byte {
	out := make([]byte, 8)
	binary.BigEndian.PutUint64(out, value)
	return out
}

func join(parts ...[]byte) []byte {
	var out []byte
	for _, part := range parts {
		out = append(out, part...)
	}
	return out
}

// SequencerKey orders authorizations of one sequencer by first batch.
func SequencerKey(sequencerID [32]byte, firstBatch uint64) []byte {
	return join(SequencerPrefix, sequencerID[:], u64(firstBatch))
}

func SequencerIDPrefix(sequencerID [32]byte) []byte { return join(SequencerPrefix, sequencerID[:]) }

func GuarantorKey(guarantorID [32]byte) []byte { return join(GuarantorPrefix, guarantorID[:]) }

func UnbondingKey(id uint64) []byte { return join(UnbondingPrefix, u64(id)) }

func CheckpointKey(batchNumber uint64) []byte { return join(CheckpointPrefix, u64(batchNumber)) }

// CheckpointIDKey indexes the batch a checkpoint identifier was recorded for.
func CheckpointIDKey(checkpointID [32]byte) []byte { return join(CheckpointIDPrefix, checkpointID[:]) }

func AvailabilityKey(batchNumber uint64, guarantorID [32]byte) []byte {
	return join(AvailabilityPrefix, u64(batchNumber), guarantorID[:])
}

func AvailabilityBatchPrefix(batchNumber uint64) []byte {
	return join(AvailabilityPrefix, u64(batchNumber))
}

func ChallengeKey(id uint64) []byte { return join(ChallengePrefix, u64(id)) }

// SlashRecordKey is unique per guarantor, reason and batch, so one offence is
// slashed once.
func SlashRecordKey(guarantorID [32]byte, reason uint8, batchNumber uint64) []byte {
	return join(SlashRecordPrefix, guarantorID[:], []byte{reason}, u64(batchNumber))
}
