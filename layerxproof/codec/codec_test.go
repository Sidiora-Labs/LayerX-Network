package codec_test

import (
	"bufio"
	"bytes"
	"encoding/hex"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sidiora-labs/paxeer-network/layerxproof/codec"
	"github.com/sidiora-labs/paxeer-network/layerxproof/testvectors"
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

func TestDomainTagsAreNulTerminatedAndDistinct(t *testing.T) {
	seen := map[string]bool{}
	for d := codec.Domain(0); d < 20; d++ {
		tag, err := d.Tag()
		if err != nil {
			t.Fatalf("domain %d: %v", d, err)
		}
		if len(tag) < 2 || tag[len(tag)-1] != 0 || bytes.IndexByte(tag, 0) != len(tag)-1 {
			t.Fatalf("domain %d tag %q is not singly NUL-terminated", d, tag)
		}
		if seen[string(tag)] {
			t.Fatalf("domain %d tag %q repeats", d, tag)
		}
		seen[string(tag)] = true
	}
	if _, err := codec.Domain(20).Tag(); err == nil {
		t.Fatal("domain 20 accepted")
	}
}

func TestReceiptDecodeMatchesRustCanonicalForm(t *testing.T) {
	for _, v := range load(t, "receipts") {
		raw := field(t, v, "receipt")
		receipt, err := codec.DecodeReceipt(raw)
		if !v.Valid {
			continue
		}
		if err != nil {
			t.Fatalf("%s: %v", v.Name, err)
		}
		if !bytes.Equal(receipt.CanonicalBytes(), raw) {
			t.Fatalf("%s: canonical bytes differ", v.Name)
		}
		if !bytes.Equal(receipt.UnsignedBytes(), field(t, v, "unsigned")) {
			t.Fatalf("%s: unsigned form differs from Rust encode_unsigned", v.Name)
		}
		if receipt.Digest() != array(t, v, "digest") {
			t.Fatalf("%s: digest differs from Rust receipt_digest", v.Name)
		}
		if receipt.ActivityID != array(t, v, "activity_id") ||
			receipt.Asset != array(t, v, "asset") ||
			receipt.ResultingStateRoot != array(t, v, "resulting_state_root") {
			t.Fatalf("%s: decoded identity fields differ", v.Name)
		}
		for key, got := range map[string]uint64{
			"global_sequence": receipt.GlobalSequence,
			"module_id":       uint64(receipt.ModuleID),
			"operation":       uint64(receipt.Operation),
			"amount":          receipt.Amount.Lo,
			"timestamp":       receipt.Timestamp,
		} {
			want, err := v.Uint64(key)
			if err != nil {
				t.Fatal(err)
			}
			if got != want {
				t.Fatalf("%s: %s = %d, want %d", v.Name, key, got, want)
			}
		}
	}
}

func TestReceiptDecodeRefusals(t *testing.T) {
	want := map[string]error{
		"truncated":      codec.ErrTruncated,
		"truncated-half": codec.ErrTruncated,
		// As in layerx-wire, a byte after the signature makes the tail long
		// enough to be read as a program outcome, which is what refuses it.
		"trailing-byte":          codec.ErrNonCanonical,
		"oversize-digest-length": codec.ErrLengthLimit,
		"oversize-effect-count":  codec.ErrLengthLimit,
		"oversize-effect-body":   codec.ErrLengthLimit,
		"unknown-structure-tag":  codec.ErrReceiptShape,
	}
	matched := 0
	for _, v := range load(t, "receipts") {
		expected, ok := want[v.Name]
		if !ok {
			continue
		}
		matched++
		if _, err := codec.DecodeReceipt(field(t, v, "receipt")); !errors.Is(err, expected) {
			t.Fatalf("%s: got %v, want %v", v.Name, err, expected)
		}
	}
	if matched != len(want) {
		t.Fatalf("fixture carries %d of %d decode refusals", matched, len(want))
	}
	for _, v := range load(t, "receipts") {
		if v.Name != "valid-0" {
			continue
		}
		unsigned := append(field(t, v, "unsigned"), 0)
		if _, err := codec.DecodeReceipt(unsigned); !errors.Is(err, codec.ErrTrailingBytes) {
			t.Fatalf("byte after an unsigned receipt: got %v", err)
		}
	}
}

func TestStateWitnessAgainstRustFixture(t *testing.T) {
	for _, v := range load(t, "state") {
		root := array(t, v, "state_root")
		witness, err := codec.DecodeStateWitness(field(t, v, "witness"))
		if err == nil {
			err = witness.Verify(root)
		}
		if (err == nil) != v.Valid {
			t.Fatalf("%s: Go verdict %v, Rust valid=%v", v.Name, err, v.Valid)
		}
		if v.Valid {
			module, _ := v.Uint64("module_id")
			if uint64(witness.ModuleID) != module ||
				!bytes.Equal(witness.Key, field(t, v, "key")) ||
				!bytes.Equal(witness.Value, field(t, v, "value")) {
				t.Fatalf("%s: proven leaf differs", v.Name)
			}
		}
	}
}

func TestStateWitnessRefusalClasses(t *testing.T) {
	want := map[string]error{
		"wrong-root":               codec.ErrStateRoot,
		"mutated-value":            codec.ErrStateRoot,
		"truncated":                codec.ErrStateEncoding,
		"trailing-byte":            codec.ErrStateEncoding,
		"unsupported-version":      codec.ErrStateVersion,
		"module-out-of-range":      codec.ErrStateModule,
		"oversize-key-length":      codec.ErrStateEncoding,
		"oversize-value-length":    codec.ErrStateEncoding,
		"oversize-path-depth":      codec.ErrStatePath,
		"forged-promotion-sibling": codec.ErrStatePath,
		"index-outside-subtree":    codec.ErrStatePath,
		"module-tree-width":        codec.ErrStateModule,
	}
	matched := 0
	for _, v := range load(t, "state") {
		expected, ok := want[v.Name]
		if !ok {
			continue
		}
		matched++
		witness, err := codec.DecodeStateWitness(field(t, v, "witness"))
		if err == nil {
			err = witness.Verify(array(t, v, "state_root"))
		}
		if !errors.Is(err, expected) {
			t.Fatalf("%s: got %v, want %v", v.Name, err, expected)
		}
	}
	if matched != len(want) {
		t.Fatalf("fixture carries %d of %d state refusals", matched, len(want))
	}
}

func TestAccountValueAgainstRustFixture(t *testing.T) {
	for _, v := range load(t, "accounts") {
		account, err := codec.DecodeAccountValue(array(t, v, "account_id"), field(t, v, "value"))
		if (err == nil) != v.Valid {
			t.Fatalf("%s: Go verdict %v, Rust valid=%v", v.Name, err, v.Valid)
		}
		if v.Valid {
			balance, _ := v.Uint64("balance")
			if !account.HasAsset || account.AssetID != array(t, v, "asset_id") ||
				account.Balance.Hi != 0 || account.Balance.Lo != balance {
				t.Fatalf("%s: decoded account differs", v.Name)
			}
		}
	}
}

// The C core produced contracts/config/native-state-proofs.json; every entry
// must decode and fold to its committed root, and refuse a different root.
func TestNativeStateProofsFromCore(t *testing.T) {
	root, err := testvectors.RepositoryRoot()
	if err != nil {
		t.Fatal(err)
	}
	raw, err := os.ReadFile(filepath.Join(root, "contracts", "config", "native-state-proofs.json"))
	if err != nil {
		t.Fatal(err)
	}
	var document struct {
		Vectors []struct {
			Root  string `json:"root"`
			Proof string `json:"proof"`
		} `json:"vectors"`
	}
	if err := json.Unmarshal(raw, &document); err != nil {
		t.Fatal(err)
	}
	if len(document.Vectors) == 0 {
		t.Fatal("no core state proofs")
	}
	for index, entry := range document.Vectors {
		proof, err := hex.DecodeString(strings.TrimPrefix(entry.Proof, "0x"))
		if err != nil {
			t.Fatal(err)
		}
		rootBytes, err := hex.DecodeString(strings.TrimPrefix(entry.Root, "0x"))
		if err != nil || len(rootBytes) != 32 {
			t.Fatalf("vector %d: root", index)
		}
		var stateRoot [32]byte
		copy(stateRoot[:], rootBytes)
		witness, err := codec.DecodeStateWitness(proof)
		if err != nil {
			t.Fatalf("vector %d: %v", index, err)
		}
		if err := witness.Verify(stateRoot); err != nil {
			t.Fatalf("vector %d: %v", index, err)
		}
		stateRoot[31] ^= 1
		if err := witness.Verify(stateRoot); !errors.Is(err, codec.ErrStateRoot) {
			t.Fatalf("vector %d: wrong root gave %v", index, err)
		}
		if _, err := codec.DecodeStateWitness(append(proof, 0)); err == nil {
			t.Fatalf("vector %d: trailing byte accepted", index)
		}
	}
}

// tests/vectors/program_account_state_v2.vec is emitted by the C core.
func TestProgramAccountStateVectorFromCore(t *testing.T) {
	root, err := testvectors.RepositoryRoot()
	if err != nil {
		t.Fatal(err)
	}
	file, err := os.Open(filepath.Join(root, "tests", "vectors", "program_account_state_v2.vec"))
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()
	values := map[string][]byte{}
	scanner := bufio.NewScanner(file)
	scanner.Buffer(make([]byte, 1<<20), 1<<24)
	for scanner.Scan() {
		key, value, ok := strings.Cut(scanner.Text(), "=")
		if !ok {
			continue
		}
		if decoded, err := hex.DecodeString(value); err == nil {
			values[key] = decoded
		}
	}
	if err := scanner.Err(); err != nil {
		t.Fatal(err)
	}
	get := func(key string) [32]byte {
		var out [32]byte
		if len(values[key]) != 32 {
			t.Fatalf("core vector lacks 32-byte %s", key)
		}
		copy(out[:], values[key])
		return out
	}
	if codec.StateNodeHash(get("leaf0"), get("leaf1")) != get("node01") {
		t.Fatal("state node hash differs from core")
	}
	if codec.StateNodeHash(get("leaf2"), get("leaf2")) != get("node22") {
		t.Fatal("promoted state node differs from core")
	}
	if codec.StateNodeHash(get("node01"), get("node22")) != get("tree_root") {
		t.Fatal("state tree root differs from core")
	}
	programs := get("programs_root")
	if codec.StateLeafHash([]byte{0, 9}, programs[:]) != get("outer9_leaf") {
		t.Fatal("module leaf wrapper differs from core")
	}
	account, err := codec.DecodeAccountValue(get("account_id"), values["account_value"])
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(account.Name, values["account_name"]) || account.AssetID != get("asset_id") {
		t.Fatal("decoded core account differs")
	}
}
