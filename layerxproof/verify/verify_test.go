package verify_test

import (
	"errors"
	"testing"

	"github.com/sidiora-labs/paxeer-network/layerxproof/codec"
	"github.com/sidiora-labs/paxeer-network/layerxproof/testvectors"
	"github.com/sidiora-labs/paxeer-network/layerxproof/verify"
)

func load(t *testing.T, section string) []testvectors.Vector {
	t.Helper()
	fixture, err := testvectors.Load()
	if err != nil {
		t.Fatal(err)
	}
	cases := fixture[section]
	if len(cases) == 0 {
		t.Fatalf("fixture section %s is empty", section)
	}
	return cases
}

func field(t *testing.T, v testvectors.Vector, key string) []byte {
	t.Helper()
	raw, err := v.Bytes(key)
	if err != nil {
		t.Fatal(err)
	}
	return raw
}

func array(t *testing.T, v testvectors.Vector, key string) [32]byte {
	t.Helper()
	out, err := v.Array32(key)
	if err != nil {
		t.Fatal(err)
	}
	return out
}

func signature(t *testing.T, v testvectors.Vector, key string) [64]byte {
	t.Helper()
	raw := field(t, v, key)
	if len(raw) != 64 {
		t.Fatalf("%s: %s is %d bytes", v.Name, key, len(raw))
	}
	var out [64]byte
	copy(out[:], raw)
	return out
}

func number(t *testing.T, v testvectors.Vector, key string) uint64 {
	t.Helper()
	out, err := v.Uint64(key)
	if err != nil {
		t.Fatal(err)
	}
	return out
}

func TestEd25519MatchesRustStrictVerifier(t *testing.T) {
	for _, v := range load(t, "ed25519") {
		publicKey := array(t, v, "public_key")
		sig := signature(t, v, "signature")
		message := field(t, v, "message")
		var err error
		if _, domainSeparated := v.Fields["domain"]; domainSeparated {
			domain := codec.Domain(number(t, v, "domain"))
			digest, hashErr := codec.DomainHash(domain, message)
			if hashErr != nil {
				t.Fatalf("%s: %v", v.Name, hashErr)
			}
			if digest != array(t, v, "digest") {
				t.Fatalf("%s: domain digest differs from Rust SignatureMessage", v.Name)
			}
			err = verify.Ed25519Domain(publicKey, sig, domain, message)
		} else {
			err = verify.Ed25519(publicKey, sig, message)
		}
		if (err == nil) != v.Valid {
			t.Fatalf("%s: Go verdict %v, Rust valid=%v", v.Name, err, v.Valid)
		}
		if err != nil && !errors.Is(err, verify.ErrBadSignature) {
			t.Fatalf("%s: refusal %v is not ErrBadSignature", v.Name, err)
		}
	}
}

func TestReceiptSignatureMatchesRust(t *testing.T) {
	want := map[string]error{
		"bit-flipped-signature":   verify.ErrSequencerSignature,
		"bit-flipped-body":        verify.ErrSequencerSignature,
		"wrong-sequencer-key":     verify.ErrSequencerSignature,
		"missing-signature":       verify.ErrReceiptMissingSignature,
		"legacy-protocol-version": verify.ErrReceiptProtocolVersion,
	}
	for _, v := range load(t, "receipts") {
		verified, err := verify.ReceiptSignature(field(t, v, "receipt"), array(t, v, "public_key"))
		if (err == nil) != v.Valid {
			t.Fatalf("%s: Go verdict %v, Rust valid=%v", v.Name, err, v.Valid)
		}
		if v.Valid && verified.Digest != array(t, v, "digest") {
			t.Fatalf("%s: digest differs", v.Name)
		}
		if expected, ok := want[v.Name]; ok && !errors.Is(err, expected) {
			t.Fatalf("%s: got %v, want %v", v.Name, err, expected)
		}
	}
}

func inclusion(t *testing.T, v testvectors.Vector) (*verify.VerifiedReceipt, *verify.VerifiedBatchHeader, error) {
	t.Helper()
	proof, err := codec.DecodeMerkleProof(field(t, v, "proof"))
	if err != nil {
		return nil, nil, err
	}
	return verify.ReceiptInclusion(field(t, v, "receipt"), proof, field(t, v, "header"),
		signature(t, v, "header_signature"), verify.SequencerAuthorization{
			SequencerID:      array(t, v, "sequencer_id"),
			PublicKey:        array(t, v, "public_key"),
			FirstBatchNumber: number(t, v, "first_batch"),
			LastBatchNumber:  number(t, v, "last_batch"),
		})
}

func TestReceiptInclusionMatchesRust(t *testing.T) {
	want := map[string]error{
		"wrong-root-sibling":           codec.ErrMerkleRootMismatch,
		"proof-of-other-leaf":          codec.ErrMerkleRootMismatch,
		"truncated-proof":              codec.ErrTruncated,
		"trailing-proof-byte":          codec.ErrTrailingBytes,
		"forged-promotion-sibling":     codec.ErrMerklePromotionSibling,
		"bit-flipped-header-signature": verify.ErrHeaderSignature,
		"batch-outside-authorisation":  verify.ErrHeaderBatchNumber,
		"wrong-sequencer-identity":     verify.ErrSequencerIdentity,
	}
	matched := 0
	for _, v := range load(t, "inclusion") {
		receipt, header, err := inclusion(t, v)
		if (err == nil) != v.Valid {
			t.Fatalf("%s: Go verdict %v, Rust valid=%v", v.Name, err, v.Valid)
		}
		if expected, ok := want[v.Name]; ok {
			matched++
			if !errors.Is(err, expected) {
				t.Fatalf("%s: got %v, want %v", v.Name, err, expected)
			}
		}
		if !v.Valid {
			continue
		}
		if header.Digest != array(t, v, "header_digest") ||
			header.Header.ReceiptMerkleRoot != array(t, v, "receipt_root") ||
			header.Header.ResultingStateRoot != array(t, v, "resulting_state_root") ||
			header.Header.BatchNumber != number(t, v, "batch_number") {
			t.Fatalf("%s: verified header differs from Rust", v.Name)
		}
		if receipt.Receipt.ResultingStateRoot != header.Header.ResultingStateRoot {
			t.Fatalf("%s: receipt root is not the header root", v.Name)
		}
		proof, err := codec.DecodeMerkleProof(field(t, v, "proof"))
		if err != nil {
			t.Fatal(err)
		}
		if _, err := verify.ReceiptAtRoot(field(t, v, "receipt"), proof, header.Header.ReceiptMerkleRoot,
			array(t, v, "public_key")); err != nil {
			t.Fatalf("%s: ReceiptAtRoot: %v", v.Name, err)
		}
		wrong := header.Header.ReceiptMerkleRoot
		wrong[0] ^= 1
		if _, err := verify.ReceiptAtRoot(field(t, v, "receipt"), proof, wrong,
			array(t, v, "public_key")); !errors.Is(err, codec.ErrMerkleRootMismatch) {
			t.Fatalf("%s: wrong root gave %v", v.Name, err)
		}
	}
	if matched != len(want) {
		t.Fatalf("fixture carries %d of %d inclusion refusals", matched, len(want))
	}
}

func TestStateProofMatchesRust(t *testing.T) {
	for _, v := range load(t, "state") {
		_, err := verify.StateProof(field(t, v, "witness"), array(t, v, "state_root"))
		if (err == nil) != v.Valid {
			t.Fatalf("%s: Go verdict %v, Rust valid=%v", v.Name, err, v.Valid)
		}
	}
}

func TestAccountProofBindsIdentityAndAsset(t *testing.T) {
	var witness testvectors.Vector
	for _, v := range load(t, "state") {
		if v.Name == "valid-account" {
			witness = v
		}
	}
	var value testvectors.Vector
	for _, v := range load(t, "accounts") {
		if v.Name == "valid-account-value" {
			value = v
		}
	}
	if witness.Name == "" || value.Name == "" {
		t.Fatal("fixture lacks the account witness")
	}
	raw, root := field(t, witness, "witness"), array(t, witness, "state_root")
	accountID, asset := array(t, value, "account_id"), array(t, value, "asset_id")
	account, err := verify.AccountProof(raw, root, accountID, &asset)
	if err != nil {
		t.Fatal(err)
	}
	if account.Balance.Lo != number(t, value, "balance") {
		t.Fatal("proven balance differs")
	}
	other := asset
	other[0] ^= 1
	if _, err := verify.AccountProof(raw, root, accountID, &other); !errors.Is(err, verify.ErrAssetIdentity) {
		t.Fatalf("foreign asset gave %v", err)
	}
	stranger := accountID
	stranger[0] ^= 1
	if _, err := verify.AccountProof(raw, root, stranger, &asset); err == nil {
		t.Fatal("foreign account accepted")
	}
	for _, v := range load(t, "state") {
		if v.Name == "valid-module-promoted" {
			if _, err := verify.AccountProof(field(t, v, "witness"), array(t, v, "state_root"),
				accountID, nil); !errors.Is(err, verify.ErrNotAccountProof) {
				t.Fatalf("module witness as account gave %v", err)
			}
		}
	}
}

func TestStateProofAtHeaderBindsResultingRoot(t *testing.T) {
	var included testvectors.Vector
	for _, v := range load(t, "inclusion") {
		if v.Name == "valid-leaf-0" {
			included = v
		}
	}
	var witness testvectors.Vector
	for _, v := range load(t, "state") {
		if v.Name == "valid-account" {
			witness = v
		}
	}
	// The fixture witness folds to its own root, not the header's resulting
	// root, so the header binding must refuse it.
	_, _, err := verify.StateProofAtHeader(field(t, witness, "witness"), field(t, included, "header"),
		signature(t, included, "header_signature"), verify.SequencerAuthorization{
			SequencerID:      array(t, included, "sequencer_id"),
			PublicKey:        array(t, included, "public_key"),
			FirstBatchNumber: number(t, included, "first_batch"),
			LastBatchNumber:  number(t, included, "last_batch"),
		})
	if !errors.Is(err, codec.ErrStateRoot) && !errors.Is(err, verify.ErrStateRootBinding) {
		t.Fatalf("unbound state proof gave %v", err)
	}
}

func TestDiscoveryProofMatchesRust(t *testing.T) {
	want := map[string]error{
		"bit-flipped-signature":    verify.ErrDiscoverySignature,
		"wrong-state-root":         verify.ErrDiscoverySignature,
		"truncated-payload":        codec.ErrDiscoveryMalformed,
		"trailing-payload-byte":    codec.ErrDiscoveryMalformed,
		"truncated-proof-material": codec.ErrDiscoveryMalformed,
		"oversize-proof-material":  codec.ErrDiscoveryMalformed,
		"other-program":            verify.ErrDiscoveryProgram,
		"other-staleness-window":   verify.ErrDiscoveryFreshness,
		"untrusted-sequencer-key":  verify.ErrDiscoverySequencerKey,
		"unknown-layout-version":   codec.ErrDiscoveryMalformed,
	}
	for _, v := range load(t, "discovery") {
		verified, err := verify.DiscoveryProof(field(t, v, "payload"), field(t, v, "proof_material"),
			array(t, v, "program_id"), number(t, v, "staleness_ms"), array(t, v, "public_key"))
		if (err == nil) != v.Valid {
			t.Fatalf("%s: Go verdict %v, Rust valid=%v", v.Name, err, v.Valid)
		}
		if expected, ok := want[v.Name]; ok && !errors.Is(err, expected) {
			t.Fatalf("%s: got %v, want %v", v.Name, err, expected)
		}
		if !v.Valid {
			continue
		}
		if verified.Digest != array(t, v, "digest") ||
			verified.Head.StateRoot != array(t, v, "state_root") ||
			verified.Head.CodeHash != array(t, v, "code_hash") ||
			verified.HeadReceiptDigest != array(t, v, "head_receipt_digest") ||
			verified.Head.ObservedSequence != number(t, v, "observed_sequence") ||
			verified.Head.ValidThrough != number(t, v, "valid_through") {
			t.Fatalf("%s: verified head differs from Rust", v.Name)
		}
	}
}
