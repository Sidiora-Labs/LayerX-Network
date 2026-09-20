package verify_test

import (
	"errors"
	"testing"

	"github.com/sidiora-labs/paxeer-network/layerxproof/codec"
	"github.com/sidiora-labs/paxeer-network/layerxproof/testvectors"
	"github.com/sidiora-labs/paxeer-network/layerxproof/verify"
	"github.com/stretchr/testify/require"
)

type anchorFixture struct {
	domain   verify.SettlementDomain
	delay    uint64
	signers  map[[32]byte][20]byte
	fixture  testvectors.Fixture
	sequence verify.SequencerAuthorization
}

func loadAnchor(t *testing.T) *anchorFixture {
	t.Helper()
	fixture, err := testvectors.LoadAnchor()
	require.NoError(t, err)
	context := fixture["context"][0]
	out := &anchorFixture{fixture: fixture, signers: map[[32]byte][20]byte{}}
	out.domain.PaxeerChainID, err = context.Uint64("paxeer_chain_id")
	require.NoError(t, err)
	contract, err := context.Bytes("settlement_contract")
	require.NoError(t, err)
	copy(out.domain.SettlementContract[:], contract)
	out.delay, err = context.Uint64("maximum_attestation_delay_ms")
	require.NoError(t, err)
	out.sequence.SequencerID, err = context.Array32("sequencer_id")
	require.NoError(t, err)
	out.sequence.PublicKey, err = context.Array32("sequencer_public_key")
	require.NoError(t, err)
	out.sequence.FirstBatchNumber, out.sequence.LastBatchNumber = 1, 8
	// Guarantor 3 signs validly but is never bonded.
	for _, guarantor := range fixture["guarantors"][:3] {
		id, err := guarantor.Array32("guarantor_id")
		require.NoError(t, err)
		raw, err := guarantor.Bytes("signer")
		require.NoError(t, err)
		var signer [20]byte
		copy(signer[:], raw)
		out.signers[id] = signer
	}
	return out
}

func (a *anchorFixture) signerOf(id [32]byte, _ uint64) ([20]byte, bool) {
	signer, ok := a.signers[id]
	return signer, ok
}

func TestCheckpointCertificateVectors(t *testing.T) {
	a := loadAnchor(t)
	require.Len(t, a.fixture["checkpoints"], 13)
	for _, c := range a.fixture["checkpoints"] {
		t.Run(c.Name, func(t *testing.T) {
			header, err := c.Bytes("header")
			require.NoError(t, err)
			rawSignature, err := c.Bytes("header_signature")
			require.NoError(t, err)
			var signature [64]byte
			require.Len(t, rawSignature, 64)
			copy(signature[:], rawSignature)
			encoded, err := c.Bytes("certificate")
			require.NoError(t, err)
			refusal := c.Fields["refusal"]

			certificate, err := codec.DecodeCheckpointCertificate(encoded)
			if refusal == "unsorted" || refusal == "duplicate" {
				require.ErrorIs(t, err, codec.ErrUnsortedSequence)
				return
			}
			require.NoError(t, err)
			require.Equal(t, header, certificate.HeaderBytes)

			_, headerErr := verify.BatchHeader(header, signature, a.sequence)
			switch refusal {
			case "sequencer":
				require.Error(t, headerErr)
				return
			case "authorization":
				require.ErrorIs(t, headerErr, verify.ErrHeaderBatchNumber)
				return
			}
			require.NoError(t, headerErr)

			id, signers, err := verify.CheckpointCertificate(certificate, a.domain, a.delay, a.signerOf)
			expectedID, idErr := c.Array32("checkpoint_id")
			require.NoError(t, idErr)
			require.Equal(t, expectedID, id)
			switch refusal {
			case "signature":
				require.ErrorIs(t, err, verify.ErrGuarantorSignature)
			case "guarantor":
				require.ErrorIs(t, err, verify.ErrGuarantorUnknown)
			case "freshness":
				require.ErrorIs(t, err, verify.ErrAttestationFreshness)
			default:
				// Continuity refusals are the anchor module's, not the certificate's.
				require.NoError(t, err)
				expectedSigners, err := c.Uint64("signers")
				require.NoError(t, err)
				require.Equal(t, int(expectedSigners), signers)
				threshold, err := c.Uint64("threshold")
				require.NoError(t, err)
				require.Equal(t, uint8(threshold), certificate.Threshold)
			}
		})
	}
}

func TestCheckpointCertificateRefusesMutation(t *testing.T) {
	a := loadAnchor(t)
	encoded, err := a.fixture["checkpoints"][0].Bytes("certificate")
	require.NoError(t, err)
	for _, length := range []int{0, 1, len(encoded) - 1} {
		_, err := codec.DecodeCheckpointCertificate(encoded[:length])
		require.Error(t, err)
	}
	_, err = codec.DecodeCheckpointCertificate(append(append([]byte(nil), encoded...), 0))
	require.Error(t, err)

	// Any flipped byte of a signed statement breaks the certificate.
	const firstAttestation = 2 + 4 + codec.BatchHeaderBytes + 4 + 0 + 1
	certificate, err := codec.DecodeCheckpointCertificate(encoded)
	require.NoError(t, err)
	offset := firstAttestation + len(certificate.ValidityProof)
	for _, delta := range []int{0, 14, 42, 180, 181, 209, 273} {
		mutated := append([]byte(nil), encoded...)
		mutated[offset+delta] ^= 0x01
		decoded, err := codec.DecodeCheckpointCertificate(mutated)
		if err != nil {
			continue
		}
		_, _, err = verify.CheckpointCertificate(decoded, a.domain, a.delay, a.signerOf)
		require.Error(t, err, "delta %d", delta)
	}

	wrongDomain := a.domain
	wrongDomain.PaxeerChainID++
	require.NotNil(t, certificate.Settlement)
	_, _, err = verify.CheckpointCertificate(certificate, wrongDomain, a.delay, a.signerOf)
	require.ErrorIs(t, err, verify.ErrSettlementReference)

	// The same certificate before settlement carries no reference; the wrong
	// domain is then refused by the attestations themselves.
	unsettled := append([]byte(nil), encoded[:len(encoded)-codec.SettlementReferenceBytes-2]...)
	unsettled = append(unsettled, 0, 0)
	pending, err := codec.DecodeCheckpointCertificate(unsettled)
	require.NoError(t, err)
	require.Nil(t, pending.Settlement)
	_, signers, err := verify.CheckpointCertificate(pending, a.domain, a.delay, a.signerOf)
	require.NoError(t, err)
	require.Equal(t, 3, signers)
	_, _, err = verify.CheckpointCertificate(pending, wrongDomain, a.delay, a.signerOf)
	require.ErrorIs(t, err, verify.ErrAttestationDomain)
}

func TestGuarantorEquivocationVectors(t *testing.T) {
	a := loadAnchor(t)
	byName := map[string]*codec.GuarantorAttestation{}
	for _, v := range a.fixture["attestations"] {
		raw, err := v.Bytes("attestation")
		require.NoError(t, err)
		attestation, err := codec.DecodeGuarantorAttestation(raw)
		require.NoError(t, err)
		require.NoError(t, verify.GuarantorSignature(attestation))
		expected, err := v.Array32("guarantor_id")
		require.NoError(t, err)
		require.Equal(t, expected, attestation.GuarantorID)
		signer, err := v.Bytes("signer")
		require.NoError(t, err)
		require.Equal(t, signer, attestation.Signer[:])
		byName[v.Name] = attestation
	}
	first, conflicting := byName["batch_1_attestation_0"], byName["batch_1_conflicting_attestation_0"]
	require.NoError(t, verify.GuarantorEquivocation(first, conflicting, a.domain))
	require.NoError(t, verify.GuarantorEquivocation(conflicting, first, a.domain))
	require.ErrorIs(t, verify.GuarantorEquivocation(first, first, a.domain), verify.ErrEquivocationShape)
	require.ErrorIs(t, verify.GuarantorEquivocation(first, byName["batch_1_conflicting_attestation_1"], a.domain),
		verify.ErrEquivocationShape)
	wrongDomain := a.domain
	wrongDomain.SettlementContract[0] ^= 1
	require.ErrorIs(t, verify.GuarantorEquivocation(first, conflicting, wrongDomain), verify.ErrAttestationDomain)

	raw, err := a.fixture["attestations"][0].Bytes("attestation")
	require.NoError(t, err)
	raw[209] ^= 0x01
	forged, err := codec.DecodeGuarantorAttestation(raw)
	require.NoError(t, err)
	require.True(t, errors.Is(verify.GuarantorEquivocation(forged, conflicting, a.domain), verify.ErrGuarantorSignature))
}
