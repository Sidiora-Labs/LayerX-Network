package layerx

import (
	"encoding/hex"
	"encoding/json"
	"os"
	"testing"
)

const programOutcomeV3Vector = "505247330100000000000100010000000700000001000000000000000b000000000000000c000000000000000d000000000000000e00000001000000000000000f0000000000000000000000000000000000000000000000000000000000000000000000000000000100000000000000020000000000000003000000000000000400000000000000050000000000000006000000000000000700000020000000000000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000010000000201111111111111111111111111111111111111111111111111111111111111111000000202222222222222222222222222222222222222222222222222222222222222222000000200000000000000000000000000000000000000000000000000000000000000000"

func TestProgramOutcomeV3Vector(t *testing.T) {
	encoded := fixtureBytes(t, programOutcomeV3Vector)
	outcome, err := DecodeProgramReceiptOutcome(encoded, 1)
	if err != nil {
		t.Fatalf("decode Programs outcome: %v", err)
	}
	if outcome.EncodingVersion != 3 || outcome.ABIVersion != 1 || outcome.FeeUnits != NewUint128(0, 16) {
		t.Fatalf("Programs outcome scalar fields diverged")
	}
	var callGraphRoot, terminalPayloadRoot [32]byte
	for index := range callGraphRoot {
		callGraphRoot[index] = 0x11
		terminalPayloadRoot[index] = 0x22
	}
	if outcome.CallGraphRoot != callGraphRoot || outcome.TerminalPayloadRoot != terminalPayloadRoot {
		t.Fatalf("Programs outcome roots diverged")
	}
}

type receiptFixtureAuthority struct {
	BatchIDHex            string `json:"batch_id_hex"`
	AssetHex              string `json:"asset_hex"`
	PreviousStateRootHex  string `json:"previous_state_root_hex"`
	ResultingStateRootHex string `json:"resulting_state_root_hex"`
	SequencerPublicKeyHex string `json:"sequencer_public_key_hex"`
}

type receiptFixture struct {
	CanonicalReceiptHex string                  `json:"canonical_receipt_hex"`
	AuthorizedBatch     receiptFixtureAuthority `json:"authorized_batch"`
	Expected            struct {
		Level             string `json:"level"`
		ResultCode        int32  `json:"result_code"`
		ProtocolVersion   uint16 `json:"protocol_version"`
		Operation         uint8  `json:"operation"`
		ModuleID          uint16 `json:"module_id"`
		GlobalSequence    uint64 `json:"global_sequence"`
		TimestampMs       uint64 `json:"timestamp_ms"`
		Amount            string `json:"amount"`
		FeeCharged        string `json:"fee_charged"`
		FromBalanceBefore string `json:"from_balance_before"`
		FromBalanceAfter  string `json:"from_balance_after"`
		ToBalanceBefore   string `json:"to_balance_before"`
		ToBalanceAfter    string `json:"to_balance_after"`
		ActivityIDHex     string `json:"activity_id_hex"`
		FromHex           string `json:"from_hex"`
		ToHex             string `json:"to_hex"`
		ReceiptDigestHex  string `json:"receipt_digest_hex"`
	} `json:"expected"`
}

func loadReceiptFixtureNamed(t *testing.T, name string) receiptFixture {
	t.Helper()
	raw, err := os.ReadFile("../conformance/fixtures/" + name)
	if err != nil {
		t.Fatalf("read fixture: %v", err)
	}
	var fixture receiptFixture
	if err := json.Unmarshal(raw, &fixture); err != nil {
		t.Fatalf("decode fixture: %v", err)
	}
	return fixture
}

func loadReceiptFixture(t *testing.T) receiptFixture {
	t.Helper()
	return loadReceiptFixtureNamed(t, "receipt-positive-v2.json")
}

func fixtureBytes(t *testing.T, value string) []byte {
	t.Helper()
	decoded, err := hex.DecodeString(value)
	if err != nil {
		t.Fatalf("decode hex: %v", err)
	}
	return decoded
}

func fixture32(t *testing.T, value string) [32]byte {
	t.Helper()
	decoded := fixtureBytes(t, value)
	if len(decoded) != 32 {
		t.Fatalf("expected 32 bytes, got %d", len(decoded))
	}
	var out [32]byte
	copy(out[:], decoded)
	return out
}

func fixtureUint128(t *testing.T, value string) Uint128 {
	t.Helper()
	parsed, err := ParseUint128(value)
	if err != nil {
		t.Fatalf("parse u128 %q: %v", value, err)
	}
	return parsed
}

func fixtureAuthorizedBatch(t *testing.T, fixture receiptFixture) AuthorizedBatch {
	t.Helper()
	return AuthorizedBatch{
		BatchID:            fixture32(t, fixture.AuthorizedBatch.BatchIDHex),
		Asset:              fixture32(t, fixture.AuthorizedBatch.AssetHex),
		PreviousStateRoot:  fixture32(t, fixture.AuthorizedBatch.PreviousStateRootHex),
		ResultingStateRoot: fixture32(t, fixture.AuthorizedBatch.ResultingStateRootHex),
		SequencerPublicKey: fixture32(t, fixture.AuthorizedBatch.SequencerPublicKeyHex),
	}
}

func fixtureAuthority(t *testing.T, authority receiptFixtureAuthority) AuthorizedBatch {
	t.Helper()
	return AuthorizedBatch{
		BatchID:            fixture32(t, authority.BatchIDHex),
		Asset:              fixture32(t, authority.AssetHex),
		PreviousStateRoot:  fixture32(t, authority.PreviousStateRootHex),
		ResultingStateRoot: fixture32(t, authority.ResultingStateRootHex),
		SequencerPublicKey: fixture32(t, authority.SequencerPublicKeyHex),
	}
}

func TestVerifyReceiptFixturePositive(t *testing.T) {
	fixture := loadReceiptFixture(t)
	canonical := fixtureBytes(t, fixture.CanonicalReceiptHex)
	verified, err := VerifyReceipt(canonical, fixtureAuthorizedBatch(t, fixture))
	if err != nil {
		t.Fatalf("verify canonical core receipt: %v", err)
	}
	if verified.Level != fixture.Expected.Level {
		t.Fatalf("level %q, want %q", verified.Level, fixture.Expected.Level)
	}
	receipt := verified.Receipt
	if receipt.ResultCode != fixture.Expected.ResultCode {
		t.Fatalf("result code %d, want %d", receipt.ResultCode, fixture.Expected.ResultCode)
	}
	if receipt.ProtocolVersion != fixture.Expected.ProtocolVersion {
		t.Fatalf("protocol version %d, want %d", receipt.ProtocolVersion, fixture.Expected.ProtocolVersion)
	}
	if receipt.Operation != fixture.Expected.Operation {
		t.Fatalf("operation %d, want %d", receipt.Operation, fixture.Expected.Operation)
	}
	if receipt.ModuleID != fixture.Expected.ModuleID {
		t.Fatalf("module %d, want %d", receipt.ModuleID, fixture.Expected.ModuleID)
	}
	if receipt.GlobalSequence != fixture.Expected.GlobalSequence {
		t.Fatalf("global sequence %d, want %d", receipt.GlobalSequence, fixture.Expected.GlobalSequence)
	}
	if receipt.Timestamp != fixture.Expected.TimestampMs {
		t.Fatalf("timestamp %d, want %d", receipt.Timestamp, fixture.Expected.TimestampMs)
	}
	if !receipt.Amount.Equal(fixtureUint128(t, fixture.Expected.Amount)) {
		t.Fatalf("amount %s, want %s", receipt.Amount.String(), fixture.Expected.Amount)
	}
	if !receipt.FeeCharged.Equal(fixtureUint128(t, fixture.Expected.FeeCharged)) {
		t.Fatalf("fee %s, want %s", receipt.FeeCharged.String(), fixture.Expected.FeeCharged)
	}
	if !receipt.FromBalanceBefore.Equal(fixtureUint128(t, fixture.Expected.FromBalanceBefore)) {
		t.Fatalf("from before %s, want %s", receipt.FromBalanceBefore.String(), fixture.Expected.FromBalanceBefore)
	}
	if !receipt.FromBalanceAfter.Equal(fixtureUint128(t, fixture.Expected.FromBalanceAfter)) {
		t.Fatalf("from after %s, want %s", receipt.FromBalanceAfter.String(), fixture.Expected.FromBalanceAfter)
	}
	if !receipt.ToBalanceBefore.Equal(fixtureUint128(t, fixture.Expected.ToBalanceBefore)) {
		t.Fatalf("to before %s, want %s", receipt.ToBalanceBefore.String(), fixture.Expected.ToBalanceBefore)
	}
	if !receipt.ToBalanceAfter.Equal(fixtureUint128(t, fixture.Expected.ToBalanceAfter)) {
		t.Fatalf("to after %s, want %s", receipt.ToBalanceAfter.String(), fixture.Expected.ToBalanceAfter)
	}
	if receipt.ActivityID != fixture32(t, fixture.Expected.ActivityIDHex) {
		t.Fatalf("activity id mismatch")
	}
	if receipt.From != fixture32(t, fixture.Expected.FromHex) {
		t.Fatalf("from account mismatch")
	}
	if receipt.To != fixture32(t, fixture.Expected.ToHex) {
		t.Fatalf("to account mismatch")
	}
	if receipt.BatchID != fixture32(t, fixture.AuthorizedBatch.BatchIDHex) {
		t.Fatalf("batch id mismatch")
	}
	if receipt.Asset != fixture32(t, fixture.AuthorizedBatch.AssetHex) {
		t.Fatalf("asset mismatch")
	}
	if verified.ReceiptDigest != fixture32(t, fixture.Expected.ReceiptDigestHex) {
		t.Fatalf("receipt digest %x, want %s", verified.ReceiptDigest, fixture.Expected.ReceiptDigestHex)
	}
	if !verified.Facts.Amount.Equal(fixtureUint128(t, fixture.Expected.Amount)) {
		t.Fatalf("facts amount %s, want %s", verified.Facts.Amount.String(), fixture.Expected.Amount)
	}
	if verified.Facts.ResultCode != fixture.Expected.ResultCode {
		t.Fatalf("facts result code %d, want %d", verified.Facts.ResultCode, fixture.Expected.ResultCode)
	}
}

func TestVerifyReceiptFixtureByteFlipFails(t *testing.T) {
	fixture := loadReceiptFixture(t)
	canonical := fixtureBytes(t, fixture.CanonicalReceiptHex)
	mutated := make([]byte, len(canonical))
	copy(mutated, canonical)
	mutated[len(mutated)-1] ^= 0x01
	if _, err := VerifyReceipt(mutated, fixtureAuthorizedBatch(t, fixture)); err == nil {
		t.Fatalf("mutated receipt verified; a flipped signature byte must fail")
	}
}

func TestVerifyProgramsReceiptFixturePreservesOutcome(t *testing.T) {
	fixture := loadReceiptFixtureNamed(t, "receipt-programs-positive-v2.json")
	verified, err := VerifyReceipt(
		fixtureBytes(t, fixture.CanonicalReceiptHex),
		fixtureAuthorizedBatch(t, fixture),
	)
	if err != nil {
		t.Fatalf("verify Programs receipt: %v", err)
	}
	outcome := verified.Receipt.ProgramOutcome
	if outcome == nil {
		t.Fatalf("Programs receipt lost its optional outcome")
	}
	if outcome.EncodingVersion != 3 || outcome.RuntimeVersion != 1 || outcome.ABIVersion != 1 || outcome.FeeUnits != NewUint128(0, 16) {
		t.Fatalf("Programs receipt outcome diverged")
	}
	if outcome.OccupancyByteBatches != NewUint128(0, 2) || outcome.OccupancyFeeUnits != NewUint128(0, 7) {
		t.Fatalf("Programs occupancy totals diverged")
	}
	if outcome.OccupancyAssetID != fixture32(t, fixture.AuthorizedBatch.AssetHex) || outcome.OccupancyEvidenceDigest == [32]byte{} || outcome.OccupancyTransferRoot == [32]byte{} {
		t.Fatalf("Programs occupancy evidence diverged")
	}
}

func TestReceiptRefusalVectorsExposeSharedTaxonomy(t *testing.T) {
	var fixture struct {
		AuthorizedBatch receiptFixtureAuthority `json:"authorized_batch"`
		Vectors         []struct {
			Name                string `json:"name"`
			ExpectedCheck       string `json:"expected_check"`
			CanonicalReceiptHex string `json:"canonical_receipt_hex"`
		} `json:"vectors"`
	}
	raw, err := os.ReadFile("../conformance/fixtures/receipt-refusals-v2.json")
	if err != nil {
		t.Fatalf("read refusal fixture: %v", err)
	}
	if err := json.Unmarshal(raw, &fixture); err != nil {
		t.Fatalf("decode refusal fixture: %v", err)
	}
	for _, vector := range fixture.Vectors {
		vector := vector
		t.Run(vector.Name, func(t *testing.T) {
			_, err := VerifyReceipt(
				fixtureBytes(t, vector.CanonicalReceiptHex),
				fixtureAuthority(t, fixture.AuthorizedBatch),
			)
			failure, ok := err.(*VerificationError)
			if !ok {
				t.Fatalf("untyped receipt failure: %T %v", err, err)
			}
			if string(failure.Check) != vector.ExpectedCheck {
				t.Fatalf("receipt check %q, want %q", failure.Check, vector.ExpectedCheck)
			}
		})
	}
}

func TestExplicitProtocolThree(t *testing.T) {
	for _, name := range []string{"receipt-positive-v3.json", "receipt-programs-positive-v3.json"} {
		fixture := loadReceiptFixtureNamed(t, name)
		canonical := fixtureBytes(t, fixture.CanonicalReceiptHex)
		a := fixture.AuthorizedBatch
		authority := AuthorizedBatch{BatchID: fixture32(t, a.BatchIDHex), Asset: fixture32(t, a.AssetHex), PreviousStateRoot: fixture32(t, a.PreviousStateRootHex), ResultingStateRoot: fixture32(t, a.ResultingStateRootHex), SequencerPublicKey: fixture32(t, a.SequencerPublicKeyHex)}
		if _, err := VerifyReceipt(canonical, authority); err == nil {
			t.Fatal("default accepted protocol 3")
		}
		verified, err := VerifyReceipt(canonical, authority, 3)
		if err != nil {
			t.Fatal(err)
		}
		if verified.Receipt.ProtocolVersion != 3 || hex.EncodeToString(verified.ReceiptDigest[:]) != fixture.Expected.ReceiptDigestHex {
			t.Fatal("protocol 3 evidence mismatch")
		}
		canonical[len(canonical)-1] ^= 1
		if _, err := VerifyReceipt(canonical, authority, 3); err == nil {
			t.Fatal("corrupt signature accepted")
		}
	}
}
