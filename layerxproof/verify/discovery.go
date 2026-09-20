package verify

import (
	"errors"

	"github.com/sidiora-labs/paxeer-network/layerxproof/codec"
)

var (
	ErrDiscoveryRequest      = errors.New("layerx verify: discovery request identity")
	ErrDiscoveryProgram      = errors.New("layerx verify: discovery program mismatch")
	ErrDiscoveryFreshness    = errors.New("layerx verify: discovery freshness binding")
	ErrDiscoverySequencerKey = errors.New("layerx verify: discovery sequencer key mismatch")
	ErrDiscoverySignature    = errors.New("layerx verify: discovery signature")
)

// VerifiedDiscovery is a program head attestation verified under the trusted
// sequencer key. Callers compare Head.ValidThrough with their own clock.
type VerifiedDiscovery struct {
	Head              codec.ProgramDiscoveryHead
	HeadReceiptDigest [32]byte
	Digest            [32]byte
}

// DiscoveryProof verifies the sequencer's program discovery evidence with the
// refusals of layerx-client decode_program_head_attestation: exact layout,
// the requested program, valid_through == observed_at + stalenessMs, the
// trusted sequencer key, and a strict signature over the discovery digest.
func DiscoveryProof(payload, proofMaterial []byte, programID [32]byte, stalenessMs uint64,
	sequencerPublicKey [32]byte) (*VerifiedDiscovery, error) {
	if programID == ([32]byte{}) || stalenessMs == 0 {
		return nil, ErrDiscoveryRequest
	}
	attestation, err := codec.DecodeProgramHeadAttestation(payload, proofMaterial)
	if err != nil {
		return nil, err
	}
	if attestation.Head.ProgramID != programID {
		return nil, ErrDiscoveryProgram
	}
	validThrough := attestation.Head.ObservedAt + stalenessMs
	if validThrough < attestation.Head.ObservedAt || validThrough != attestation.Head.ValidThrough {
		return nil, ErrDiscoveryFreshness
	}
	if attestation.PublicKey != sequencerPublicKey {
		return nil, ErrDiscoverySequencerKey
	}
	digest := attestation.Head.Digest()
	if err := Ed25519Digest(sequencerPublicKey, attestation.Signature, digest); err != nil {
		return nil, ErrDiscoverySignature
	}
	return &VerifiedDiscovery{Head: attestation.Head, HeadReceiptDigest: attestation.HeadReceiptDigest, Digest: digest}, nil
}
