package verify

import (
	"errors"
	"math/big"

	"github.com/ethereum/go-ethereum/crypto"
	"github.com/sidiora-labs/paxeer-network/layerxproof/codec"
)

var (
	ErrAttestationBinding   = errors.New("layerx verify: attestation does not bind the checkpoint")
	ErrAttestationDomain    = errors.New("layerx verify: attestation settlement domain")
	ErrAttestationDuty      = errors.New("layerx verify: attestation replay or availability duty")
	ErrAttestationFreshness = errors.New("layerx verify: attestation outside the freshness window")
	ErrGuarantorSignature   = errors.New("layerx verify: guarantor signature")
	ErrGuarantorUnknown     = errors.New("layerx verify: guarantor not bonded and active")
	ErrSettlementReference  = errors.New("layerx verify: settlement reference")
	ErrEquivocationShape    = errors.New("layerx verify: statements do not contradict")
)

var secp256k1HalfOrder = new(big.Int).Rsh(crypto.S256().Params().N, 1)

// SettlementDomain is the Paxeer chain and settlement address every
// attestation must name, as block.chainid and the bond contract do in
// CheckpointRegistry.registerCheckpoint.
type SettlementDomain struct {
	PaxeerChainID      uint64
	SettlementContract [20]byte
}

// GuarantorSigner resolves the signer address a bonded, active guarantor is
// authorised to attest with at the given epoch.
type GuarantorSigner func(guarantorID [32]byte, epoch uint64) ([20]byte, bool)

// GuarantorSignature checks the attestation's secp256k1 signature exactly as
// GuarantorBond._validSignature and lxp_guarantor_attestation_verify do: v is
// 27 or 28, S is in the lower half order, and the recovered address is the
// declared, non-zero signer.
func GuarantorSignature(attestation *codec.GuarantorAttestation) error {
	if attestation.SignatureV != 27 && attestation.SignatureV != 28 {
		return ErrGuarantorSignature
	}
	if attestation.Signer == ([20]byte{}) {
		return ErrGuarantorSignature
	}
	r := new(big.Int).SetBytes(attestation.Signature[:32])
	s := new(big.Int).SetBytes(attestation.Signature[32:])
	if r.Sign() == 0 || s.Sign() == 0 || s.Cmp(secp256k1HalfOrder) > 0 {
		return ErrGuarantorSignature
	}
	digest := attestation.Digest()
	var compact [65]byte
	copy(compact[:64], attestation.Signature[:])
	compact[64] = attestation.SignatureV - 27
	public, err := crypto.SigToPub(digest[:], compact[:])
	if err != nil {
		return ErrGuarantorSignature
	}
	if crypto.PubkeyToAddress(*public) != attestation.Signer {
		return ErrGuarantorSignature
	}
	return nil
}

func attestationBinds(attestation *codec.GuarantorAttestation, header *codec.BatchHeader, checkpointID [32]byte,
	domain SettlementDomain, maximumDelayMs uint64) error {
	if attestation.ProtocolVersion != header.ProtocolVersion || attestation.NetworkID != header.NetworkID ||
		attestation.Epoch != header.Epoch || attestation.BatchNumber != header.BatchNumber ||
		attestation.CheckpointID != checkpointID || attestation.CheckpointHash != checkpointID ||
		attestation.DataAvailabilityRoot != header.DataAvailabilityRoot {
		return ErrAttestationBinding
	}
	if attestation.PaxeerChainID != domain.PaxeerChainID || attestation.SettlementContract != domain.SettlementContract {
		return ErrAttestationDomain
	}
	if attestation.AttestedAtMs < header.TimestampMs || attestation.AttestedAtMs-header.TimestampMs > maximumDelayMs {
		return ErrAttestationFreshness
	}
	return nil
}

// Attestation verifies one guarantor attestation against the checkpoint it
// names: binding, settlement domain, freshness, a possessed availability mask
// inside the five classes, signature, and the guarantor's authorised signer.
func Attestation(attestation *codec.GuarantorAttestation, header *codec.BatchHeader, checkpointID [32]byte,
	domain SettlementDomain, maximumDelayMs uint64, signerOf GuarantorSigner) error {
	if err := attestationBinds(attestation, header, checkpointID, domain, maximumDelayMs); err != nil {
		return err
	}
	if !attestation.DataPossessed || attestation.AvailabilityClassMask == 0 ||
		attestation.AvailabilityClassMask&^codec.AvailabilityAll != 0 {
		return ErrAttestationDuty
	}
	if err := GuarantorSignature(attestation); err != nil {
		return err
	}
	signer, ok := signerOf(attestation.GuarantorID, header.Epoch)
	if !ok || signer != attestation.Signer {
		return ErrGuarantorUnknown
	}
	return nil
}

// CheckpointCertificate verifies a decoded certificate with the rule of
// CheckpointRegistry.registerCheckpoint: every attestation binds the
// checkpoint, names the settlement domain, replayed the batch, possesses all
// five availability classes, is fresh, is validly signed, and comes from a
// bonded active guarantor. Ordering and distinctness were enforced by the
// decoder. It returns the checkpoint identifier and the count of verified
// attestations; comparing that count and the declared threshold with the
// required quorum is the caller's finality decision.
func CheckpointCertificate(certificate *codec.CheckpointCertificate, domain SettlementDomain, maximumDelayMs uint64,
	signerOf GuarantorSigner) ([32]byte, int, error) {
	checkpointID := certificate.CheckpointID()
	if !codec.ProtocolVersionUsesOccupancy(certificate.Header.ProtocolVersion) || certificate.Header.NetworkID == 0 ||
		certificate.Header.Epoch == 0 || certificate.Header.BatchNumber == 0 {
		return checkpointID, 0, ErrHeaderCanonical
	}
	if certificate.Settlement != nil && (certificate.Settlement.PaxeerChainID != domain.PaxeerChainID ||
		certificate.Settlement.SettlementContract != domain.SettlementContract ||
		certificate.Settlement.CheckpointID != checkpointID) {
		return checkpointID, 0, ErrSettlementReference
	}
	for _, attestation := range certificate.Attestations {
		if !attestation.Replayed || attestation.AvailabilityClassMask != codec.AvailabilityAll {
			return checkpointID, 0, ErrAttestationDuty
		}
		if err := Attestation(attestation, certificate.Header, checkpointID, domain, maximumDelayMs, signerOf); err != nil {
			return checkpointID, 0, err
		}
	}
	return checkpointID, len(certificate.Attestations), nil
}

// GuarantorEquivocation verifies that two attestations are a contradiction by
// one guarantor (lxp_equivocation.c guarantor_contradiction and
// GuarantorBond.submitEquivocation): identical protocol, network, settlement
// domain, epoch, batch and guarantor, each naming its own checkpoint hash,
// different checkpoint hashes, and both validly signed by the same signer.
func GuarantorEquivocation(first, second *codec.GuarantorAttestation, domain SettlementDomain) error {
	if first.ProtocolVersion != second.ProtocolVersion || first.NetworkID != second.NetworkID ||
		first.PaxeerChainID != second.PaxeerChainID || first.SettlementContract != second.SettlementContract ||
		first.Epoch != second.Epoch || first.BatchNumber != second.BatchNumber ||
		first.GuarantorID != second.GuarantorID || first.CheckpointID != first.CheckpointHash ||
		second.CheckpointID != second.CheckpointHash || first.CheckpointHash == second.CheckpointHash ||
		first.Signer != second.Signer {
		return ErrEquivocationShape
	}
	if first.PaxeerChainID != domain.PaxeerChainID || first.SettlementContract != domain.SettlementContract {
		return ErrAttestationDomain
	}
	if err := GuarantorSignature(first); err != nil {
		return err
	}
	return GuarantorSignature(second)
}
