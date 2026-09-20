package verify_test

import (
	"bytes"
	"testing"

	"github.com/sidiora-labs/paxeer-network/layerxproof/codec"
	"github.com/sidiora-labs/paxeer-network/layerxproof/verify"
)

func TestWithdrawalOutcomeMatchesRustVerifier(t *testing.T) {
	for _, v := range load(t, "withdrawal") {
		_, effect, err := verify.WithdrawalReceipt(field(t, v, "receipt"), array(t, v, "public_key"))
		if (err == nil) != v.Valid {
			t.Fatalf("%s: valid=%v, got %v", v.Name, v.Valid, err)
		}
		if !v.Valid {
			continue
		}
		amount := effect.Amount.Bytes()
		recipient := field(t, v, "recipient")
		if uint64(effect.NetworkID) != number(t, v, "network_id") ||
			effect.WithdrawalID != array(t, v, "withdrawal_id") ||
			effect.Account != array(t, v, "account") ||
			effect.Asset != array(t, v, "asset") ||
			effect.Amount.Hi != 0 || effect.Amount.Lo != number(t, v, "amount") || len(amount) != 16 ||
			!bytes.Equal(effect.Recipient[:], recipient) ||
			effect.Anchor != array(t, v, "anchor") ||
			effect.Nullifier != array(t, v, "nullifier") ||
			effect.FeeLimit != number(t, v, "fee_limit") {
			t.Fatalf("%s: decoded withdrawal differs from the Rust facts: %+v", v.Name, effect)
		}
	}
}

func TestWithdrawalInclusionAgainstRealBatch(t *testing.T) {
	v := load(t, "withdrawal")[0]
	proof, err := codec.DecodeMerkleProof(field(t, v, "proof"))
	if err != nil {
		t.Fatal(err)
	}
	batch := number(t, v, "batch_number")
	authorization := verify.SequencerAuthorization{SequencerID: array(t, v, "sequencer_id"),
		PublicKey: array(t, v, "public_key"), FirstBatchNumber: batch, LastBatchNumber: batch}
	_, header, effect, err := verify.WithdrawalInclusion(field(t, v, "receipt"), proof, field(t, v, "header"),
		signature(t, v, "header_signature"), authorization)
	if err != nil {
		t.Fatal(err)
	}
	if header.Header.ResultingStateRoot != array(t, v, "header_state_root") ||
		header.Header.ReceiptMerkleRoot != array(t, v, "header_receipt_root") ||
		uint64(header.Header.NetworkID) != number(t, v, "header_network_id") ||
		effect.Nullifier != array(t, v, "nullifier") {
		t.Fatalf("verified header or effect differs from the Rust facts")
	}
	authorization.FirstBatchNumber, authorization.LastBatchNumber = batch+1, batch+1
	if _, _, _, err := verify.WithdrawalInclusion(field(t, v, "receipt"), proof, field(t, v, "header"),
		signature(t, v, "header_signature"), authorization); err == nil {
		t.Fatal("a batch outside the authorised range was accepted")
	}
}

func TestExitBalanceMatchesRustVerifier(t *testing.T) {
	for _, v := range load(t, "exit") {
		var recipient [20]byte
		copy(recipient[:], field(t, v, "recipient"))
		root := array(t, v, "state_root")
		networkID := uint32(number(t, v, "network_id")) //nolint:gosec
		message := verify.ExitRecipientMessage(networkID, array(t, v, "account"), array(t, v, "asset"), recipient, root)
		if !bytes.Equal(message, field(t, v, "message")) {
			t.Fatalf("%s: recipient message differs from the Rust preimage", v.Name)
		}
		account, err := verify.ExitBalance(field(t, v, "witness"), root, networkID, array(t, v, "account"),
			array(t, v, "asset"), recipient, root, signature(t, v, "recipient_signature"))
		if (err == nil) != v.Valid {
			t.Fatalf("%s: valid=%v, got %v", v.Name, v.Valid, err)
		}
		if v.Valid && (account.Balance.Hi != 0 || account.Balance.Lo != number(t, v, "balance") ||
			account.AuthorityKey != array(t, v, "authority")) {
			t.Fatalf("%s: proven account differs from the Rust facts", v.Name)
		}
		if v.Valid {
			recipient[0] ^= 1
			if _, err := verify.ExitBalance(field(t, v, "witness"), root, networkID, array(t, v, "account"),
				array(t, v, "asset"), recipient, root, signature(t, v, "recipient_signature")); err == nil {
				t.Fatalf("%s: a recipient the authority did not sign was accepted", v.Name)
			}
		}
	}
}
