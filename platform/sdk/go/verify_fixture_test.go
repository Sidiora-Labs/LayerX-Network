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

type receiptFixture struct {
	CanonicalReceiptHex string `json:"canonical_receipt_hex"`
	AuthorizedBatch     struct {
		BatchIDHex            string `json:"batch_id_hex"`
		AssetHex              string `json:"asset_hex"`
		PreviousStateRootHex  string `json:"previous_state_root_hex"`
		ResultingStateRootHex string `json:"resulting_state_root_hex"`
		SequencerPublicKeyHex string `json:"sequencer_public_key_hex"`
	} `json:"authorized_batch"`
	Expected struct {
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

func loadReceiptFixture(t *testing.T) receiptFixture {
	t.Helper()
	raw, err := os.ReadFile("../conformance/fixtures/receipt-positive-v1.json")
	if err != nil {
		t.Fatalf("read fixture: %v", err)
	}
	var fixture receiptFixture
	if err := json.Unmarshal(raw, &fixture); err != nil {
		t.Fatalf("decode fixture: %v", err)
	}
	return fixture
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
