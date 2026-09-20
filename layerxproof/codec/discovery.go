package codec

import (
	"crypto/sha256"
	"encoding/binary"
	"errors"
)

const (
	programHeadAttestVersion uint16 = 1
	// ProgramHeadAttestPayloadBytes is the exact attestation payload width.
	ProgramHeadAttestPayloadBytes = 2 + 32 + 4 + 32 + 2 + 8 + 8 + 8 + 32 + 32
	// ProgramHeadAttestProofBytes is the exact proof-material width.
	ProgramHeadAttestProofBytes = 32 + 64
)

// ErrDiscoveryMalformed refuses a discovery attestation with a wrong layout.
var ErrDiscoveryMalformed = errors.New("layerx discovery: malformed response")

// ProgramDiscoveryHead is the exact fact set bound by the discovery digest.
type ProgramDiscoveryHead struct {
	ProgramID        [32]byte
	Version          uint32
	CodeHash         [32]byte
	AbiVersion       uint16
	ObservedSequence uint64
	ObservedAt       uint64
	ValidThrough     uint64
	StateRoot        [32]byte
}

// ProgramHeadAttestation is a decoded, not yet verified, head attestation.
type ProgramHeadAttestation struct {
	Head              ProgramDiscoveryHead
	HeadReceiptDigest [32]byte
	PublicKey         [32]byte
	Signature         [64]byte
}

// Digest computes the LayerX/program-discovery-proof/v1 digest.
func (h *ProgramDiscoveryHead) Digest() [32]byte {
	var scalars [4 + 2 + 8 + 8 + 8]byte
	hash := sha256.New()
	hash.Write([]byte(ProgramDiscoveryProofDomain))
	hash.Write(h.ProgramID[:])
	hash.Write([]byte{1})
	binary.BigEndian.PutUint32(scalars[:4], h.Version)
	hash.Write(scalars[:4])
	hash.Write(h.CodeHash[:])
	binary.BigEndian.PutUint16(scalars[4:6], h.AbiVersion)
	binary.BigEndian.PutUint64(scalars[6:14], h.ObservedSequence)
	binary.BigEndian.PutUint64(scalars[14:22], h.ObservedAt)
	binary.BigEndian.PutUint64(scalars[22:30], h.ValidThrough)
	hash.Write(scalars[4:])
	hash.Write(h.StateRoot[:])
	var out [32]byte
	hash.Sum(out[:0])
	return out
}

// DecodeProgramHeadAttestation decodes the LNI program head attestation
// response payload and proof material with their exact fixed layout.
func DecodeProgramHeadAttestation(payload, proofMaterial []byte) (*ProgramHeadAttestation, error) {
	if len(payload) != ProgramHeadAttestPayloadBytes || len(proofMaterial) != ProgramHeadAttestProofBytes ||
		binary.BigEndian.Uint16(payload[:2]) != programHeadAttestVersion {
		return nil, ErrDiscoveryMalformed
	}
	a := &ProgramHeadAttestation{}
	copy(a.Head.ProgramID[:], payload[2:34])
	a.Head.Version = binary.BigEndian.Uint32(payload[34:38])
	copy(a.Head.CodeHash[:], payload[38:70])
	a.Head.AbiVersion = binary.BigEndian.Uint16(payload[70:72])
	a.Head.ObservedSequence = binary.BigEndian.Uint64(payload[72:80])
	a.Head.ObservedAt = binary.BigEndian.Uint64(payload[80:88])
	a.Head.ValidThrough = binary.BigEndian.Uint64(payload[88:96])
	copy(a.Head.StateRoot[:], payload[96:128])
	copy(a.HeadReceiptDigest[:], payload[128:])
	copy(a.PublicKey[:], proofMaterial[:32])
	copy(a.Signature[:], proofMaterial[32:])
	zero := [32]byte{}
	if a.Head.Version == 0 || a.Head.ObservedSequence == 0 || a.Head.ObservedAt == 0 ||
		a.Head.StateRoot == zero || a.HeadReceiptDigest == zero {
		return nil, ErrDiscoveryMalformed
	}
	return a, nil
}
