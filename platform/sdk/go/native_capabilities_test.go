package layerx

import (
	"bytes"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"os"
	"testing"
)

type nativeCapabilityFixtureGrant struct {
	Tag           byte   `json:"tag"`
	Kind          string `json:"kind"`
	Program       string `json:"program"`
	OwnerProgram  string `json:"owner_program"`
	Seed          string `json:"seed"`
	SourceAccount string `json:"source_account"`
	Asset         string `json:"asset"`
	To            string `json:"to"`
	Account       string `json:"account"`
	ReceiptDigest string `json:"receipt_digest"`
	MaximumAmount string `json:"maximum_amount"`
}

func nativeCapabilityFixtureGrants(t *testing.T, values []nativeCapabilityFixtureGrant) []NativeCapability {
	t.Helper()
	field := func(value string) [32]byte {
		decoded, err := hex.DecodeString(value)
		if err != nil || len(decoded) != 32 {
			t.Fatalf("invalid identifier %q", value)
		}
		var result [32]byte
		copy(result[:], decoded)
		return result
	}
	amount := func(value string) Uint128 {
		parsed, err := ParseUint128(value)
		if err != nil {
			t.Fatal(err)
		}
		return parsed
	}
	grants := make([]NativeCapability, 0, len(values))
	for _, value := range values {
		var grant NativeCapability
		switch value.Kind {
		case "StorageRead":
			grant = NativeStorageRead{}
		case "StorageWrite":
			grant = NativeStorageWrite{}
		case "EmitEvent":
			grant = NativeEmitEvent{}
		case "Call":
			grant = NativeCallCapability{field(value.Program)}
		case "Transfer402":
			grant = NativeTransfer402{field(value.Asset), field(value.To), amount(value.MaximumAmount)}
		case "ProgramSpend":
			seed, err := hex.DecodeString(value.Seed)
			if err != nil {
				t.Fatal(err)
			}
			grant = NativeProgramSpend{field(value.OwnerProgram), seed, field(value.SourceAccount), field(value.Asset), field(value.To), amount(value.MaximumAmount)}
		case "ReceiptRead":
			grant = NativeReceiptRead{field(value.ReceiptDigest)}
		case "BalanceView":
			grant = NativeBalanceView{field(value.Account), field(value.Asset), field(value.ReceiptDigest)}
		case "SharedStorageRead":
			grant = NativeSharedStorageRead{}
		case "SharedStorageWrite":
			grant = NativeSharedStorageWrite{}
		default:
			t.Fatalf("unknown fixture capability %q", value.Kind)
		}
		entry, err := nativeCapabilityEntryFor(grant)
		if err != nil || entry.encoded[0] != value.Tag {
			t.Fatalf("fixture tag differs: %v", err)
		}
		grants = append(grants, grant)
	}
	return grants
}

func TestNativeCapabilityRuntimeFixture(t *testing.T) {
	raw, err := os.ReadFile("../conformance/fixtures/native-program-capabilities-v2.json")
	if err != nil {
		t.Fatal(err)
	}
	var fixture struct {
		Capabilities []nativeCapabilityFixtureGrant `json:"capabilities"`
		Canonical    string                         `json:"canonical_hex"`
		Narrowed     []nativeCapabilityFixtureGrant `json:"narrowed_capabilities"`
		NarrowedHex  string                         `json:"narrowed_hex"`
		Equal        bool                           `json:"equal_narrowing_accepted"`
		Escalations  []struct {
			Name         string                         `json:"name"`
			Parent       string                         `json:"parent"`
			Capabilities []nativeCapabilityFixtureGrant `json:"capabilities"`
			Canonical    string                         `json:"canonical_hex"`
			Accepted     bool                           `json:"accepted"`
		} `json:"escalation_cases"`
	}
	if err := json.Unmarshal(raw, &fixture); err != nil {
		t.Fatal(err)
	}
	if len(fixture.Capabilities) != 10 || len(fixture.Escalations) != 3 || !fixture.Equal {
		t.Fatal("runtime coverage differs")
	}
	for index, tag := range []byte{1, 2, 3, 4, 5, 9, 6, 10, 7, 8} {
		if fixture.Capabilities[index].Tag != tag {
			t.Fatal("canonical runtime order differs")
		}
	}
	parent := nativeCapabilityFixtureGrants(t, fixture.Capabilities)
	assertEncoding := func(grants []NativeCapability, expected string) {
		t.Helper()
		encoded, err := EncodeNativeCapabilities(grants)
		if err != nil || hex.EncodeToString(encoded) != expected {
			t.Fatalf("runtime encoding differs: %v", err)
		}
		decoded, err := DecodeNativeCapabilities(encoded)
		if err != nil {
			t.Fatal(err)
		}
		reencoded, err := EncodeNativeCapabilities(decoded)
		if err != nil || !bytes.Equal(encoded, reencoded) {
			t.Fatalf("reencode differs: %v", err)
		}
	}
	assertEncoding(parent, fixture.Canonical)
	requested := nativeCapabilityFixtureGrants(t, fixture.Narrowed)
	assertEncoding(requested, fixture.NarrowedHex)
	narrowed, err := NarrowNativeCapabilities(parent, requested)
	if err != nil {
		t.Fatal(err)
	}
	assertEncoding(narrowed, fixture.NarrowedHex)
	equal, err := NarrowNativeCapabilities(parent, parent)
	if err != nil {
		t.Fatal(err)
	}
	assertEncoding(equal, fixture.Canonical)
	for _, escalation := range fixture.Escalations {
		if escalation.Parent != "narrowed" || escalation.Accepted {
			t.Fatal("runtime escalation metadata differs")
		}
		grants := nativeCapabilityFixtureGrants(t, escalation.Capabilities)
		assertEncoding(grants, escalation.Canonical)
		if _, err := NarrowNativeCapabilities(narrowed, grants); err == nil {
			t.Fatalf("escalation accepted: %s", escalation.Name)
		}
	}
}

func capabilityIdentifier(value byte) [32]byte {
	var identifier [32]byte
	for index := range identifier {
		identifier[index] = value
	}
	return identifier
}

func TestNativeCapabilityBoundsAndNarrowing(t *testing.T) {
	owner, asset, to, receipt := capabilityIdentifier(1), capabilityIdentifier(2), capabilityIdentifier(3), capabilityIdentifier(4)
	seed := []byte{0, 255}
	source, err := DeriveNativeProgramAccount(owner, seed)
	if err != nil {
		t.Fatal(err)
	}
	maximum := NewUint128(1, 0)
	spend := NativeProgramSpend{owner, seed, source, asset, to, maximum}
	view := NativeBalanceView{source, asset, receipt}
	parent := []NativeCapability{NativeSharedStorageWrite{}, NativeSharedStorageRead{}, view, NativeReceiptRead{receipt}, spend, NativeTransfer402{asset, to, maximum}, NativeCallCapability{owner}, NativeEmitEvent{}, NativeStorageWrite{}, NativeStorageRead{}}
	encoded, err := EncodeNativeCapabilities(parent)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(encoded[:5], []byte{0, 10, 1, 2, 3}) {
		t.Fatal("wrong canonical prefix")
	}
	childSpend := spend
	childSpend.MaximumAmount = NewUint128(0, ^uint64(0))
	child, err := NarrowNativeCapabilities(parent, []NativeCapability{childSpend, view})
	if err != nil || len(child) != 2 {
		t.Fatalf("amount narrowing refused: %v", err)
	}
	childSpend.MaximumAmount = NewUint128(1, 1)
	if _, err := NarrowNativeCapabilities(parent, []NativeCapability{childSpend}); err == nil {
		t.Fatal("amount escalation accepted")
	}
	changedView := view
	changedView.ReceiptDigest = capabilityIdentifier(5)
	if _, err := NarrowNativeCapabilities(parent, []NativeCapability{changedView}); err == nil {
		t.Fatal("receipt substitution accepted")
	}
	if _, err := EncodeNativeCapabilities([]NativeCapability{view, changedView}); err == nil {
		t.Fatal("duplicate balance key accepted")
	}
	if _, err := NarrowNativeCapabilities(nil, []NativeCapability{NativeStorageRead{}}); err == nil {
		t.Fatal("missing authority accepted")
	}
	if _, err := EncodeNativeCapabilities([]NativeCapability{NativeTransfer402{asset, to, Uint128{}}}); err == nil {
		t.Fatal("zero amount accepted")
	}
	if _, err := EncodeNativeCapabilities([]NativeCapability{NativeCallCapability{}}); err == nil {
		t.Fatal("zero program accepted")
	}
	changedSpend := spend
	changedSpend.SourceAccount[0] ^= 1
	if _, err := EncodeNativeCapabilities([]NativeCapability{changedSpend}); err == nil {
		t.Fatal("forged program account accepted")
	}
	changedSpend = spend
	changedSpend.Seed = make([]byte, 129)
	if _, err := EncodeNativeCapabilities([]NativeCapability{changedSpend}); err == nil {
		t.Fatal("oversized seed accepted")
	}
	for length := 0; length < len(encoded); length++ {
		if _, err := DecodeNativeCapabilities(encoded[:length]); err == nil {
			t.Fatalf("accepted prefix %d", length)
		}
	}
	if _, err := DecodeNativeCapabilities(append(append([]byte{}, encoded...), 0)); err == nil {
		t.Fatal("accepted trailing bytes")
	}
	for _, malformed := range [][]byte{{0, 2, 2, 1}, {0, 2, 1, 1}, {0, 1, 11}, {0, 239}, {0, 1, 9}} {
		if _, err := DecodeNativeCapabilities(malformed); err == nil {
			t.Fatalf("malformed set accepted: %x", malformed)
		}
	}
	views := make([]NativeCapability, 0, 33)
	for index := byte(1); index <= 33; index++ {
		views = append(views, NativeBalanceView{capabilityIdentifier(index), asset, receipt})
	}
	if _, err := EncodeNativeCapabilities(views[:32]); err != nil {
		t.Fatal(err)
	}
	if _, err := EncodeNativeCapabilities(views); err == nil {
		t.Fatal("33 balance views accepted")
	}
	full := make([]NativeCapability, 0, MaximumNativeCapabilities+1)
	for index := 0; index <= MaximumNativeCapabilities; index++ {
		fullSeed := make([]byte, 128)
		binary.BigEndian.PutUint16(fullSeed, uint16(index))
		account, err := DeriveNativeProgramAccount(owner, fullSeed)
		if err != nil {
			t.Fatal(err)
		}
		full = append(full, NativeProgramSpend{owner, fullSeed, account, asset, to, maximum})
	}
	maximumBytes, err := EncodeNativeCapabilities(full[:MaximumNativeCapabilities])
	if err != nil || len(maximumBytes) != MaximumNativeCapabilityBytes {
		t.Fatalf("maximum canonical set: %v", err)
	}
	if _, err := EncodeNativeCapabilities(full); err == nil {
		t.Fatal("239 grants accepted")
	}
	if !validProgramCapabilities(encoded, true) || validProgramCapabilities(encoded, false) {
		t.Fatal("ABI v2 gate differs")
	}
}
