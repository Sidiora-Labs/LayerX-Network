package codec

import (
	"bytes"
	"crypto/sha256"
	"encoding/binary"
)

const (
	// CheckpointWireVersion is the evidence wire version of the checkpoint
	// payload and of the settlement reference (EVIDENCE_WIRE_VERSION).
	CheckpointWireVersion uint16 = 1
	// MaxValidityProofBytes is LXP_MAX_VALIDITY_PROOF_BYTES.
	MaxValidityProofBytes = 1_048_576
	// MaxGuarantorAttestations is LXP_MAX_GUARANTOR_ATTESTATIONS.
	MaxGuarantorAttestations = 32
	// MaxSettlementReferenceBytes is LXP_MAX_SETTLEMENT_REFERENCE_BYTES.
	MaxSettlementReferenceBytes = 1024
	// SettlementReferenceBytes is the exact width of a present reference.
	SettlementReferenceBytes = 110
	// GuarantorAttestationBytes is the wire width of one attestation: the
	// 189-byte signed statement, the signer address, the signature and v.
	GuarantorAttestationBytes = 274
	// GuarantorAttestationMessageBytes is LXP_ATTESTATION_MESSAGE_BYTES.
	GuarantorAttestationMessageBytes = 189
	// AvailabilityClassCount is LXP_DA_CLASS_COUNT.
	AvailabilityClassCount = 5
	// AvailabilityAll is LXP_GUARANTOR_AVAILABILITY_ALL: activities, receipts,
	// oracle inputs, state diff and recovery metadata.
	AvailabilityAll uint8 = 0x1f
)

// Availability classes of lxp_da_class, as bits of the attestation mask.
const (
	AvailabilityActivities       uint8 = 1 << 0
	AvailabilityReceipts         uint8 = 1 << 1
	AvailabilityOracleInputs     uint8 = 1 << 2
	AvailabilityStateDiff        uint8 = 1 << 3
	AvailabilityRecoveryMetadata uint8 = 1 << 4
)

// GuarantorAttestation is one guarantor's signed statement over a checkpoint
// (lxp_guarantor_attestation). The signature is secp256k1 over
// SHA256("LXP/v2/guarantor-attestation\0" || 189-byte statement) with an EVM
// recovery identifier, so the signer is recoverable with ecrecover.
type GuarantorAttestation struct {
	ProtocolVersion       uint16
	NetworkID             uint32
	PaxeerChainID         uint64
	SettlementContract    [20]byte
	Epoch                 uint64
	CheckpointID          [32]byte
	CheckpointHash        [32]byte
	GuarantorID           [32]byte
	BatchNumber           uint64
	DataAvailabilityRoot  [32]byte
	Replayed              bool
	DataPossessed         bool
	AvailabilityClassMask uint8
	AttestedAtMs          uint64
	Signer                [20]byte
	Signature             [64]byte
	SignatureV            uint8
	message               [GuarantorAttestationMessageBytes]byte
}

// SigningMessage returns the exact 189-byte signed statement.
func (a *GuarantorAttestation) SigningMessage() []byte {
	return append([]byte(nil), a.message[:]...)
}

// Digest is the secp256k1 signing digest of the statement.
func (a *GuarantorAttestation) Digest() [32]byte {
	return mustDomainHash(DomainGuarantorAttestation, a.message[:])
}

func wireBoolean(value byte) (bool, error) {
	switch value {
	case 0:
		return false, nil
	case 1:
		return true, nil
	default:
		return false, ErrNonCanonical
	}
}

func decodeAttestation(r *reader) (*GuarantorAttestation, error) {
	raw, err := r.take(GuarantorAttestationBytes)
	if err != nil {
		return nil, err
	}
	a := &GuarantorAttestation{}
	copy(a.message[:], raw[:GuarantorAttestationMessageBytes])
	a.ProtocolVersion = binary.BigEndian.Uint16(raw[0:2])
	a.NetworkID = binary.BigEndian.Uint32(raw[2:6])
	a.PaxeerChainID = binary.BigEndian.Uint64(raw[6:14])
	copy(a.SettlementContract[:], raw[14:34])
	a.Epoch = binary.BigEndian.Uint64(raw[34:42])
	copy(a.CheckpointID[:], raw[42:74])
	copy(a.CheckpointHash[:], raw[74:106])
	copy(a.GuarantorID[:], raw[106:138])
	a.BatchNumber = binary.BigEndian.Uint64(raw[138:146])
	copy(a.DataAvailabilityRoot[:], raw[146:178])
	if a.Replayed, err = wireBoolean(raw[178]); err != nil {
		return nil, err
	}
	if a.DataPossessed, err = wireBoolean(raw[179]); err != nil {
		return nil, err
	}
	a.AvailabilityClassMask = raw[180]
	a.AttestedAtMs = binary.BigEndian.Uint64(raw[181:189])
	copy(a.Signer[:], raw[189:209])
	copy(a.Signature[:], raw[209:273])
	a.SignatureV = raw[273]
	return a, nil
}

// DecodeGuarantorAttestation strictly decodes one 274-byte wire attestation.
func DecodeGuarantorAttestation(encoded []byte) (*GuarantorAttestation, error) {
	r := &reader{bytes: encoded}
	a, err := decodeAttestation(r)
	if err != nil {
		return nil, err
	}
	if err := r.finish(); err != nil {
		return nil, err
	}
	return a, nil
}

// SettlementReference is the 110-byte Paxeer settlement reference a
// certificate may carry once its registration was observed.
type SettlementReference struct {
	PaxeerChainID       uint64
	SettlementContract  [20]byte
	CheckpointID        [32]byte
	TransactionID       [32]byte
	ObservedBlockNumber uint64
	ObservedAtMs        uint64
}

// DecodeSettlementReference mirrors settlement_reference_decode.
func DecodeSettlementReference(encoded []byte) (*SettlementReference, error) {
	if len(encoded) != SettlementReferenceBytes {
		return nil, ErrNonCanonical
	}
	if binary.BigEndian.Uint16(encoded[0:2]) != CheckpointWireVersion {
		return nil, ErrVersionUnsupported
	}
	s := &SettlementReference{}
	s.PaxeerChainID = binary.BigEndian.Uint64(encoded[2:10])
	copy(s.SettlementContract[:], encoded[10:30])
	copy(s.CheckpointID[:], encoded[30:62])
	copy(s.TransactionID[:], encoded[62:94])
	s.ObservedBlockNumber = binary.BigEndian.Uint64(encoded[94:102])
	s.ObservedAtMs = binary.BigEndian.Uint64(encoded[102:110])
	if s.PaxeerChainID == 0 || s.ObservedBlockNumber == 0 || s.ObservedAtMs == 0 ||
		s.SettlementContract == ([20]byte{}) || s.CheckpointID == ([32]byte{}) || s.TransactionID == ([32]byte{}) {
		return nil, ErrNonCanonical
	}
	return s, nil
}

// CheckpointCertificate is the guarantor checkpoint certificate
// (lxp_guarantor_cert in the layerxd checkpoint payload form): the canonical
// batch header, the validity proof, up to 32 attestations strictly ascending
// by guarantor identifier, the declared threshold, and the settlement
// reference, which is empty before settlement and 110 bytes after it.
type CheckpointCertificate struct {
	HeaderBytes         []byte
	Header              *BatchHeader
	ValidityProof       []byte
	Attestations        []*GuarantorAttestation
	Threshold           uint8
	SettlementReference []byte
	Settlement          *SettlementReference
}

// CheckpointHash is lxp_checkpoint_certificate_hash:
// SHA256("LXP/v2/checkpoint-certificate\0" || header || u32be(len(proof)) || proof).
func CheckpointHash(header, validityProof []byte) [32]byte {
	var length [4]byte
	binary.BigEndian.PutUint32(length[:], uint32(len(validityProof))) //nolint:gosec
	h := sha256.New()
	h.Write([]byte(domainTags[DomainCheckpointCertificate]))
	h.Write(header)
	h.Write(length[:])
	h.Write(validityProof)
	var out [32]byte
	h.Sum(out[:0])
	return out
}

// CheckpointID returns the certificate's checkpoint hash.
func (c *CheckpointCertificate) CheckpointID() [32]byte {
	return CheckpointHash(c.HeaderBytes, c.ValidityProof)
}

// DecodeCheckpointCertificate strictly decodes the checkpoint payload as
// decode_checkpoint_payload does, except that the settlement reference may
// also be empty: a certificate submitted for settlement cannot yet name the
// transaction that settles it.
func DecodeCheckpointCertificate(encoded []byte) (*CheckpointCertificate, error) {
	r := &reader{bytes: encoded}
	version, err := r.u16()
	if err != nil {
		return nil, err
	}
	if version != CheckpointWireVersion {
		return nil, ErrVersionUnsupported
	}
	headerBytes, err := r.prefixed(BatchHeaderBytes)
	if err != nil {
		return nil, err
	}
	if len(headerBytes) != BatchHeaderBytes {
		return nil, ErrNonCanonical
	}
	header, err := DecodeBatchHeader(headerBytes)
	if err != nil {
		return nil, err
	}
	validity, err := r.prefixed(MaxValidityProofBytes)
	if err != nil {
		return nil, err
	}
	count, err := r.u8()
	if err != nil {
		return nil, err
	}
	if count == 0 || count > MaxGuarantorAttestations {
		return nil, ErrLengthLimit
	}
	c := &CheckpointCertificate{
		HeaderBytes:   append([]byte(nil), headerBytes...),
		Header:        header,
		ValidityProof: append([]byte(nil), validity...),
	}
	for index := 0; index < int(count); index++ {
		attestation, err := decodeAttestation(r)
		if err != nil {
			return nil, err
		}
		if index != 0 && bytes.Compare(c.Attestations[index-1].GuarantorID[:], attestation.GuarantorID[:]) >= 0 {
			return nil, ErrUnsortedSequence
		}
		c.Attestations = append(c.Attestations, attestation)
	}
	if c.Threshold, err = r.u8(); err != nil {
		return nil, err
	}
	if c.Threshold == 0 || c.Threshold > count {
		return nil, ErrNonCanonical
	}
	referenceLength, err := r.u16()
	if err != nil {
		return nil, err
	}
	if referenceLength > MaxSettlementReferenceBytes {
		return nil, ErrLengthLimit
	}
	reference, err := r.take(int(referenceLength))
	if err != nil {
		return nil, err
	}
	if err := r.finish(); err != nil {
		return nil, err
	}
	if len(reference) != 0 {
		if c.Settlement, err = DecodeSettlementReference(reference); err != nil {
			return nil, err
		}
		c.SettlementReference = append([]byte(nil), reference...)
	}
	return c, nil
}
