package codec

const (
	batchTag    uint16 = 0x1701
	batchFields uint8  = 15
	// BatchHeaderBytes is the exact canonical batch header width.
	BatchHeaderBytes = 354
)

// BatchHeader is the canonical batch header with its fifteen commitments. A
// sequencer-signed header is the checkpoint a receipt, activity or state leaf
// is proven against.
type BatchHeader struct {
	ProtocolVersion      uint16
	NetworkID            uint32
	Epoch                uint64
	BatchNumber          uint64
	FirstSequence        uint64
	LastSequence         uint64
	PreviousStateRoot    [32]byte
	ResultingStateRoot   [32]byte
	ActivityMerkleRoot   [32]byte
	ReceiptMerkleRoot    [32]byte
	EventMerkleRoot      [32]byte
	DataAvailabilityRoot [32]byte
	OracleRoot           [32]byte
	TimestampMs          uint64
	SequencerID          [32]byte
}

func batchField(r *reader, expected uint8) error {
	actual, err := r.u8()
	if err != nil {
		return err
	}
	if actual != expected {
		return ErrUnknownField
	}
	return nil
}

// DecodeBatchHeader strictly decodes the exact 354-byte batch header.
func DecodeBatchHeader(bytes []byte) (*BatchHeader, error) {
	if len(bytes) > BatchHeaderBytes {
		return nil, ErrTrailingBytes
	}
	r := &reader{bytes: bytes}
	envelope, err := r.structureHeaderVersion(batchTag)
	if err != nil {
		return nil, err
	}
	fields, err := r.u8()
	if err != nil {
		return nil, err
	}
	if fields != batchFields {
		return nil, ErrNonCanonical
	}
	h := &BatchHeader{}
	if err = batchField(r, 1); err != nil {
		return nil, err
	}
	if h.ProtocolVersion, err = r.u16(); err != nil {
		return nil, err
	}
	if h.ProtocolVersion != envelope {
		return nil, ErrVersionUnsupported
	}
	if err = batchField(r, 2); err != nil {
		return nil, err
	}
	if h.NetworkID, err = r.u32(); err != nil {
		return nil, err
	}
	u64s := []*uint64{&h.Epoch, &h.BatchNumber, &h.FirstSequence, &h.LastSequence}
	for index, target := range u64s {
		if err = batchField(r, uint8(3+index)); err != nil { //nolint:gosec
			return nil, err
		}
		if *target, err = r.u64(); err != nil {
			return nil, err
		}
	}
	roots := []*[32]byte{&h.PreviousStateRoot, &h.ResultingStateRoot, &h.ActivityMerkleRoot,
		&h.ReceiptMerkleRoot, &h.EventMerkleRoot, &h.DataAvailabilityRoot, &h.OracleRoot}
	for index, target := range roots {
		if err = batchField(r, uint8(7+index)); err != nil { //nolint:gosec
			return nil, err
		}
		if *target, err = r.array32(); err != nil {
			return nil, err
		}
	}
	if err = batchField(r, 14); err != nil {
		return nil, err
	}
	if h.TimestampMs, err = r.u64(); err != nil {
		return nil, err
	}
	if err = batchField(r, 15); err != nil {
		return nil, err
	}
	if h.SequencerID, err = r.array32(); err != nil {
		return nil, err
	}
	if err = r.finish(); err != nil {
		return nil, err
	}
	return h, nil
}
