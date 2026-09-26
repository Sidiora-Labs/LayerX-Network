package proposals

import (
	"bytes"
	"crypto/ed25519"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sidiora-labs/paxeer-network/bridge/deploy/chainconfig"
	"github.com/sidiora-labs/paxeer-network/bridge/vectors"
	"github.com/sidiora-labs/paxeer-network/modules/layerxbridge/types"
	"github.com/sidiora-labs/paxeer-network/sdk/codec"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

const (
	testManifest = "testdata/attestors.json"
	ethereumPath = "testdata/ethereum.json"
	solanaPath   = "testdata/solana.json"

	governanceAuthority = "pax10d07y265gmmuvt4z0w9aw880jnsr700jxdwa9m"
	wrappedSOLAssetID   = "0xcf996523b5d068a26f0aa8a116602fe5033ee3a1"
)

func loadManifest(t *testing.T, path string) AttestorManifest {
	t.Helper()
	manifest, err := LoadAttestorManifest(path)
	if err != nil {
		t.Fatalf("loading %s: %v", path, err)
	}
	return manifest
}

func generate(t *testing.T, configPath string) Bundle {
	t.Helper()
	cfg, err := LoadChainConfig(configPath)
	if err != nil {
		t.Fatalf("loading %s: %v", configPath, err)
	}
	bundle, err := Generate(cfg, loadManifest(t, testManifest))
	if err != nil {
		t.Fatalf("generating %s: %v", configPath, err)
	}
	return bundle
}

// decodeStrict decodes one emitted body back into the module's own generated
// message through the type URL the body carries, with unknown fields
// forbidden, so a body that names another message or carries a field the
// keeper does not know fails the test rather than reaching governance.
func decodeStrict[T any, P interface {
	*T
	sdk.Msg
}](t *testing.T, name string, body []byte) T {
	t.Helper()
	var envelope map[string]json.RawMessage
	if err := json.Unmarshal(body, &envelope); err != nil {
		t.Fatalf("%s is not a JSON object: %v\n%s", name, err, body)
	}
	var typeURL string
	if err := json.Unmarshal(envelope["@type"], &typeURL); err != nil {
		t.Fatalf("%s carries no type URL: %v\n%s", name, err, body)
	}
	if want := sdk.MsgTypeURL(P(new(T))); typeURL != want {
		t.Fatalf("%s carries the type URL %q, want %q", name, typeURL, want)
	}
	msg, err := DecodeBody(body)
	if err != nil {
		t.Fatalf("decoding %s back into %s: %v\n%s", name, typeURL, err, body)
	}
	typed, ok := msg.(P)
	if !ok {
		t.Fatalf("%s decoded into %T, want %T", name, msg, P(nil))
	}
	return *typed
}

// decodeJSONStrict decodes a document that is not a module message with
// unknown fields forbidden.
func decodeJSONStrict[T any](t *testing.T, name string, body []byte) T {
	t.Helper()
	decoder := json.NewDecoder(bytes.NewReader(body))
	decoder.DisallowUnknownFields()
	var out T
	if err := decoder.Decode(&out); err != nil {
		t.Fatalf("decoding %s back into %T: %v\n%s", name, out, err, body)
	}
	if decoder.More() {
		t.Fatalf("%s carries more than one JSON document", name)
	}
	return out
}

// sameMessage requires two messages to be equal field for field: their
// protobuf encodings, which cover every field, are byte for byte the same.
func sameMessage(t *testing.T, name string, want, got codec.ProtoMarshaler) {
	t.Helper()
	wantBytes, err := messageCodec.Marshal(want)
	if err != nil {
		t.Fatalf("encoding the generated %T: %v", want, err)
	}
	gotBytes, err := messageCodec.Marshal(got)
	if err != nil {
		t.Fatalf("encoding the decoded %s: %v", name, err)
	}
	if !bytes.Equal(wantBytes, gotBytes) {
		t.Fatalf("%s decodes to %v, the generator built %v", name, got, want)
	}
}

func fieldError(t *testing.T, err error) *FieldError {
	t.Helper()
	var refusal *FieldError
	if !errors.As(err, &refusal) {
		t.Fatalf("expected a refusal naming the file and the field, got %T: %v", err, err)
	}
	return refusal
}

func TestSidioraAssetIDIsTheAddressTheChainFixes(t *testing.T) {
	if got, want := SidioraAssetID().Hex(), strings.ToLower(types.SidioraRemoteAddress); got != want {
		t.Fatalf("Sidiora asset id is %s, want %s", got, want)
	}
}

func TestSolanaChainIDIsTheReservedIdentifier(t *testing.T) {
	if SolanaChainID != 91600046870081 {
		t.Fatalf("Solana chain id is %d, want 91600046870081", SolanaChainID)
	}
}

func TestEthereumBodiesDecodeBackIntoTheModulesTypes(t *testing.T) {
	bundle := generate(t, ethereumPath)
	manifest := loadManifest(t, testManifest)
	files, err := bundle.Files()
	if err != nil {
		t.Fatalf("marshalling the bundle: %v", err)
	}
	if len(files) != 3 {
		t.Fatalf("ethereum emitted %d bodies, want 3", len(files))
	}
	wantNames := []string{
		"01-register-chain.json",
		"02-set-attestors.json",
		"03-set-cap-01-" + strings.Repeat("00", 20) + ".json",
	}
	for i, want := range wantNames {
		if files[i].Name != want {
			t.Fatalf("body %d is %s, want %s", i, files[i].Name, want)
		}
	}

	register := decodeStrict[types.MsgRegisterChain](t, files[0].Name, files[0].Body)
	if err := register.ValidateBasic(); err != nil {
		t.Fatalf("the register-chain body the keeper would reject: %v", err)
	}
	if register.Authority != governanceAuthority {
		t.Fatalf("authority is %s, want %s", register.Authority, governanceAuthority)
	}
	if register.Chain.ChainID != 1 {
		t.Fatalf("chain id is %d, want 1", register.Chain.ChainID)
	}
	if got, want := register.Chain.Vault.Hex(), "0x7a3e5c81b04d296f8e1a7c35d92b6f04e8c1a37d"; got != want {
		t.Fatalf("vault is %s, want %s", got, want)
	}
	if register.Chain.FinalityDepth != 64 {
		t.Fatalf("finality depth is %d, want 64", register.Chain.FinalityDepth)
	}
	if !register.Chain.Enabled {
		t.Fatal("the registered chain is not enabled")
	}

	attestors := decodeStrict[types.MsgSetAttestors](t, files[1].Name, files[1].Body)
	if err := attestors.ValidateBasic(); err != nil {
		t.Fatalf("the set-attestors body the keeper would reject: %v", err)
	}
	signers := manifest.Signers()
	if len(attestors.Set.Attestors) != len(signers) {
		t.Fatalf("the body carries %d attestors, the manifest records %d",
			len(attestors.Set.Attestors), len(signers))
	}
	for i, attestor := range attestors.Set.Attestors {
		if attestor.Signer != signers[i] {
			t.Fatalf("attestor %d is %s, the manifest records %s", i, attestor.Signer.Hex(), signers[i].Hex())
		}
		if !attestor.Bond.IsZero() {
			t.Fatalf("attestor %d declares the bond %s, want zero", i, attestor.Bond.String())
		}
	}
	if attestors.Set.Threshold != manifest.Threshold {
		t.Fatalf("threshold is %d, the manifest records %d", attestors.Set.Threshold, manifest.Threshold)
	}

	native := decodeStrict[types.MsgSetCap](t, files[2].Name, files[2].Body)
	if err := native.ValidateBasic(); err != nil {
		t.Fatalf("the set-cap body the keeper would reject: %v", err)
	}
	if native.Asset != (types.Address20{}) {
		t.Fatalf("the native asset id is %s, want address(0)", native.Asset.Hex())
	}
	if got, want := native.MaxPerTx.String(), "50000000000000000000"; got != want {
		t.Fatalf("per-transaction cap is %s, want %s", got, want)
	}
	if got, want := native.MaxInFlight.String(), "1000000000000000000000"; got != want {
		t.Fatalf("total cap is %s, want %s", got, want)
	}
}

func TestSolanaBodiesCarryTheNativeCapFirstAndSidiora(t *testing.T) {
	bundle := generate(t, solanaPath)
	files, err := bundle.Files()
	if err != nil {
		t.Fatalf("marshalling the bundle: %v", err)
	}
	if len(files) != 4 {
		t.Fatalf("solana emitted %d bodies, want 4", len(files))
	}

	register := decodeStrict[types.MsgRegisterChain](t, files[0].Name, files[0].Body)
	if err := register.ValidateBasic(); err != nil {
		t.Fatalf("the register-chain body the keeper would reject: %v", err)
	}
	if register.Chain.ChainID != SolanaChainID {
		t.Fatalf("chain id is %d, want %d", register.Chain.ChainID, SolanaChainID)
	}
	if got, want := register.Chain.Vault.Hex(), "0x933d35eb8a4cb086fbb20a2f727124f5b2051fdb"; got != want {
		t.Fatalf("vault handle is %s, want %s", got, want)
	}

	if err := decodeStrict[types.MsgSetAttestors](t, files[1].Name, files[1].Body).ValidateBasic(); err != nil {
		t.Fatalf("the set-attestors body the keeper would reject: %v", err)
	}

	caps := make([]types.MsgSetCap, 0, 2)
	for _, file := range files[2:] {
		msg := decodeStrict[types.MsgSetCap](t, file.Name, file.Body)
		if err := msg.ValidateBasic(); err != nil {
			t.Fatalf("the set-cap body %s the keeper would reject: %v", file.Name, err)
		}
		if msg.ChainID != SolanaChainID {
			t.Fatalf("%s names chain %d, want %d", file.Name, msg.ChainID, SolanaChainID)
		}
		caps = append(caps, msg)
	}
	if got := caps[0].Asset.Hex(); got != wrappedSOLAssetID {
		t.Fatalf("the first cap is for %s, want the wrapped SOL handle %s", got, wrappedSOLAssetID)
	}
	if caps[1].Asset != SidioraAssetID() {
		t.Fatalf("the second cap is for %s, want Sidiora's asset id %s",
			caps[1].Asset.Hex(), SidioraAssetID().Hex())
	}
	if got, want := files[3].Name, "03-set-cap-02-"+strings.TrimPrefix(SidioraAssetID().Hex(), "0x")+".json"; got != want {
		t.Fatalf("the Sidiora body is %s, want %s", got, want)
	}
}

func TestDecodedBodiesEqualTheGeneratedMessagesFieldForField(t *testing.T) {
	for _, path := range []string{ethereumPath, solanaPath} {
		t.Run(filepath.Base(path), func(t *testing.T) {
			bundle := generate(t, path)
			files, err := bundle.Files()
			if err != nil {
				t.Fatalf("marshalling the bundle: %v", err)
			}
			if len(files) != 2+len(bundle.Caps) {
				t.Fatalf("%d bodies for %d caps", len(files), len(bundle.Caps))
			}
			register := decodeStrict[types.MsgRegisterChain](t, files[0].Name, files[0].Body)
			sameMessage(t, files[0].Name, &bundle.Register, &register)
			attestors := decodeStrict[types.MsgSetAttestors](t, files[1].Name, files[1].Body)
			sameMessage(t, files[1].Name, &bundle.Attestors, &attestors)
			for i := range bundle.Caps {
				file := files[2+i]
				decoded := decodeStrict[types.MsgSetCap](t, file.Name, file.Body)
				sameMessage(t, file.Name, &bundle.Caps[i], &decoded)
			}
		})
	}
}

func TestBodiesWithAnUnknownFieldAreRefused(t *testing.T) {
	files, err := generate(t, ethereumPath).Files()
	if err != nil {
		t.Fatalf("marshalling the bundle: %v", err)
	}
	for _, testCase := range []struct {
		name string
		body []byte
		from string
		to   string
	}{
		{"message", files[0].Body, `"authority":`, `"surplus": true, "authority":`},
		{"chain", files[0].Body, `"chain_id":`, `"surplus": true, "chain_id":`},
		{"attestor set", files[1].Body, `"threshold":`, `"surplus": true, "threshold":`},
		{"cap", files[2].Body, `"max_per_tx":`, `"surplus": true, "max_per_tx":`},
	} {
		t.Run(testCase.name, func(t *testing.T) {
			if !bytes.Contains(testCase.body, []byte(testCase.from)) {
				t.Fatalf("the body carries no %s\n%s", testCase.from, testCase.body)
			}
			body := bytes.Replace(testCase.body, []byte(testCase.from), []byte(testCase.to), 1)
			if _, err := DecodeBody(body); err == nil {
				t.Fatalf("a body with an unknown field was decoded\n%s", body)
			}
		})
	}
	unknownType := bytes.Replace(files[0].Body, []byte("MsgRegisterChain"), []byte("MsgRegisterChainV2"), 1)
	if _, err := DecodeBody(unknownType); err == nil {
		t.Fatal("a body naming a message the module does not define was decoded")
	}
}

func TestEveryConfigurationPutsTheNativeCapFirst(t *testing.T) {
	for _, chain := range []struct {
		path        string
		nativeAsset string
	}{
		{ethereumPath, "0x" + strings.Repeat("00", 20)},
		{solanaPath, wrappedSOLAssetID},
	} {
		t.Run(filepath.Base(chain.path), func(t *testing.T) {
			bundle := generate(t, chain.path)
			if len(bundle.Caps) == 0 {
				t.Fatal("no caps were generated")
			}
			if got := bundle.Caps[0].Asset.Hex(); got != chain.nativeAsset {
				t.Fatalf("the first cap is for %s, want the native coin %s", got, chain.nativeAsset)
			}
			for i, msg := range bundle.Caps[1:] {
				if msg.Asset.Hex() == chain.nativeAsset {
					t.Fatalf("the native coin is also the cap at index %d", i+1)
				}
			}
		})
	}
}

func TestCommittedManifestCarriesPlaceholdersEveryToolRefuses(t *testing.T) {
	_, err := LoadAttestorManifest(filepath.Join("..", "attestors.json"))
	if err == nil {
		t.Fatal("the committed attestor manifest was accepted")
	}
	refusal := fieldError(t, err)
	if refusal.Field != "attestors[0]" {
		t.Fatalf("the refusal names %q, want attestors[0]", refusal.Field)
	}
	if !strings.Contains(refusal.Msg, "placeholder address") {
		t.Fatalf("the refusal reads %q, want a placeholder address", refusal.Msg)
	}
}

func TestManifestRefusals(t *testing.T) {
	for _, testCase := range []struct {
		file  string
		field string
		want  string
	}{
		{"attestors-descending.json", "attestors[2]", "ascending order"},
		{"attestors-threshold-above-count.json", "threshold", "above the attestor count"},
		{"attestors-unknown-field.json", fieldDocument, "unknown field"},
	} {
		t.Run(testCase.file, func(t *testing.T) {
			path := filepath.Join("testdata", testCase.file)
			if _, err := LoadAttestorManifest(path); err == nil {
				t.Fatal("the manifest was accepted")
			} else {
				refusal := fieldError(t, err)
				if refusal.File != path {
					t.Fatalf("the refusal names the file %q, want %q", refusal.File, path)
				}
				if refusal.Field != testCase.field {
					t.Fatalf("the refusal names the field %q, want %q", refusal.Field, testCase.field)
				}
				if !strings.Contains(refusal.Msg, testCase.want) {
					t.Fatalf("the refusal reads %q, want %q", refusal.Msg, testCase.want)
				}
			}
		})
	}
}

func TestChainConfigurationRefusals(t *testing.T) {
	manifest := loadManifest(t, testManifest)
	for _, testCase := range []struct {
		file  string
		field string
		want  string
	}{
		{"placeholder-authority.json", fieldAuthority, "placeholder governance authority"},
		{"zero-authority.json", fieldAuthority, "zero governance authority"},
		{"empty-authority.json", fieldAuthority, "empty governance authority"},
		{"non-governance-authority.json", fieldAuthority, "not the governance module account"},
		{"placeholder-owner.json", fieldOwner, "placeholder address"},
		{"zero-owner.json", fieldOwner, "zero address"},
		{"placeholder-solana-owner.json", fieldOwner, "placeholder key"},
		{"zero-solana-owner.json", fieldOwner, "zero key"},
		{"placeholder-vault.json", fieldVault, "placeholder address"},
		{"zero-vault.json", fieldVault, "zero address"},
		{"zero-finality-depth.json", fieldFinalityDepth, "finality depth is zero"},
		{"placeholder-attestor.json", "attestors[2]", "placeholder address"},
		{"zero-attestor.json", "attestors[0]", "zero address"},
		{"descending-attestor.json", "attestors[2]", "ascending order"},
		{"zero-threshold.json", fieldThreshold, "threshold is zero"},
		{"threshold-above-count.json", fieldThreshold, "above the attestor count"},
		{"attestor-set-differs.json", "attestors[4]", "against"},
		{"threshold-differs.json", fieldThreshold, "against the threshold"},
		{"zero-per-tx-cap.json", "assets[0].max_per_tx", "zero cap"},
		{"zero-total-cap.json", "assets[0].max_total", "zero cap"},
		{"per-tx-above-total-cap.json", "assets[0].max_per_tx", "above the total cap"},
		{"empty-assets.json", fieldAssets, "no assets"},
		{"empty-name.json", fieldName, "empty chain name"},
		{"unknown-field.json", fieldDocument, "unknown field"},
		{"native-not-first.json", "assets[0].address", "not the chain's native coin"},
		{"native-listed-twice.json", "assets[1].address", "native coin is listed twice"},
		{"duplicate-asset-id.json", "assets[1].asset_id", "already the asset id"},
		{"solana-without-sidiora.json", fieldAssets, "Sidiora's asset id"},
	} {
		t.Run(testCase.file, func(t *testing.T) {
			path := filepath.Join("testdata", "refuse", testCase.file)
			cfg, err := LoadChainConfig(path)
			if err == nil {
				_, err = Generate(cfg, manifest)
			}
			if err == nil {
				t.Fatal("the configuration was accepted")
			}
			refusal := fieldError(t, err)
			if refusal.File != path {
				t.Fatalf("the refusal names the file %q, want %q", refusal.File, path)
			}
			if refusal.Field != testCase.field {
				t.Fatalf("the refusal names the field %q, want %q", refusal.Field, testCase.field)
			}
			if !strings.Contains(refusal.Msg, testCase.want) {
				t.Fatalf("the refusal reads %q, want %q", refusal.Msg, testCase.want)
			}
		})
	}
}

func TestRunWritesEveryBodyOfTheChain(t *testing.T) {
	out := filepath.Join(t.TempDir(), "solana")
	var report bytes.Buffer
	if err := Run([]string{"-manifest", testManifest, solanaPath, out}, &report); err != nil {
		t.Fatalf("the command refused a configuration it should accept: %v", err)
	}
	entries, err := os.ReadDir(out)
	if err != nil {
		t.Fatalf("reading the output directory: %v", err)
	}
	if len(entries) != 4 {
		t.Fatalf("the command wrote %d files, want 4", len(entries))
	}
	bundle := generate(t, solanaPath)
	files, err := bundle.Files()
	if err != nil {
		t.Fatalf("marshalling the bundle: %v", err)
	}
	for _, file := range files {
		path := filepath.Join(out, file.Name)
		written, err := os.ReadFile(path)
		if err != nil {
			t.Fatalf("reading %s: %v", path, err)
		}
		if !bytes.Equal(written, file.Body) {
			t.Fatalf("%s on disk is not the body the generator built", path)
		}
		if !strings.Contains(report.String(), path) {
			t.Fatalf("the command did not report %s", path)
		}
	}
}

func TestRunWritesIntoAnExistingEmptyDirectory(t *testing.T) {
	parent := t.TempDir()
	out := filepath.Join(parent, "solana")
	if err := os.Mkdir(out, 0o755); err != nil {
		t.Fatalf("creating the output directory: %v", err)
	}
	var report bytes.Buffer
	if err := Run([]string{"-manifest", testManifest, solanaPath, out}, &report); err != nil {
		t.Fatalf("the command refused an empty output directory: %v", err)
	}
	entries, err := os.ReadDir(out)
	if err != nil {
		t.Fatalf("reading the output directory: %v", err)
	}
	if len(entries) != 4 {
		t.Fatalf("the command wrote %d files, want 4", len(entries))
	}
	siblings, err := os.ReadDir(parent)
	if err != nil {
		t.Fatalf("reading the parent directory: %v", err)
	}
	if len(siblings) != 1 || siblings[0].Name() != "solana" {
		names := make([]string, 0, len(siblings))
		for _, sibling := range siblings {
			names = append(names, sibling.Name())
		}
		t.Fatalf("the command left %v beside the output directory, want only solana", names)
	}
}

func TestRunRefusesAnOutputDirectoryThatHoldsAnything(t *testing.T) {
	out := filepath.Join(t.TempDir(), "solana")
	if err := os.Mkdir(out, 0o755); err != nil {
		t.Fatalf("creating the output directory: %v", err)
	}
	stale := filepath.Join(out, "03-set-cap-09-stale.json")
	staleBody := []byte("{}\n")
	if err := os.WriteFile(stale, staleBody, 0o644); err != nil {
		t.Fatalf("writing the stale body: %v", err)
	}
	var report bytes.Buffer
	if err := Run([]string{"-manifest", testManifest, solanaPath, out}, &report); err == nil {
		t.Fatal("the command wrote a bundle beside the body of an earlier one")
	}
	entries, err := os.ReadDir(out)
	if err != nil {
		t.Fatalf("reading the output directory: %v", err)
	}
	if len(entries) != 1 || entries[0].Name() != filepath.Base(stale) {
		t.Fatalf("the refused run changed the output directory: %d entries", len(entries))
	}
	kept, err := os.ReadFile(stale)
	if err != nil {
		t.Fatalf("reading the stale body: %v", err)
	}
	if !bytes.Equal(kept, staleBody) {
		t.Fatal("the refused run rewrote a file it found in the output directory")
	}
	siblings, err := os.ReadDir(filepath.Dir(out))
	if err != nil {
		t.Fatalf("reading the parent directory: %v", err)
	}
	if len(siblings) != 1 {
		t.Fatalf("the refused run left %d entries beside the output directory, want 1", len(siblings)-1)
	}
}

func TestRunWritesNothingWhenItRefuses(t *testing.T) {
	for _, name := range []string{
		"solana-without-sidiora.json",
		"placeholder-authority.json",
		"threshold-differs.json",
	} {
		t.Run(name, func(t *testing.T) {
			out := filepath.Join(t.TempDir(), "proposals")
			var report bytes.Buffer
			err := Run([]string{"-manifest", testManifest, filepath.Join("testdata", "refuse", name), out}, &report)
			if err == nil {
				t.Fatal("the command accepted a configuration it must refuse")
			}
			if _, statErr := os.Stat(out); !os.IsNotExist(statErr) {
				t.Fatalf("the refused run left %s behind (%v)", out, statErr)
			}
		})
	}
}

func TestRunRefusesTheCommittedPlaceholderManifest(t *testing.T) {
	out := filepath.Join(t.TempDir(), "proposals")
	var report bytes.Buffer
	err := Run([]string{"-manifest", filepath.Join("..", "attestors.json"), ethereumPath, out}, &report)
	if err == nil {
		t.Fatal("the command ran against the committed placeholder manifest")
	}
	if _, statErr := os.Stat(out); !os.IsNotExist(statErr) {
		t.Fatalf("the refused run left %s behind (%v)", out, statErr)
	}
}

func TestRunRequiresAConfigurationAndAnOutputDirectory(t *testing.T) {
	for _, args := range [][]string{
		{},
		{ethereumPath},
		{ethereumPath, "one", "two"},
	} {
		t.Run(fmt.Sprintf("%d arguments", len(args)), func(t *testing.T) {
			var report bytes.Buffer
			if err := Run(append([]string{"-manifest", testManifest}, args...), &report); err == nil {
				t.Fatalf("the command accepted %d arguments", len(args))
			}
		})
	}
}

func TestRunReportsAMissingConfiguration(t *testing.T) {
	out := filepath.Join(t.TempDir(), "proposals")
	var report bytes.Buffer
	err := Run([]string{"-manifest", testManifest, filepath.Join("testdata", "absent.json"), out}, &report)
	if err == nil {
		t.Fatal("the command accepted a configuration that does not exist")
	}
	if _, statErr := os.Stat(out); !os.IsNotExist(statErr) {
		t.Fatalf("the refused run left %s behind (%v)", out, statErr)
	}
}

// committedConfiguration is the path of a chain's committed configuration in
// the bridge's one schema, bridge/deploy/chainconfig.
func committedConfiguration(t *testing.T, chain string) string {
	t.Helper()
	path, err := chainconfig.Path(filepath.Join("..", "..", ".."), chain)
	if err != nil {
		t.Fatalf("%s has no configuration path: %v", chain, err)
	}
	return path
}

// solanaOwner is a real ed25519 public key, the kind of key the owner of the
// custody program is.
func solanaOwner() string {
	seed := sha256.Sum256([]byte("paxeer-x-bridge proposals test owner"))
	public := ed25519.NewKeyFromSeed(seed[:]).Public().(ed25519.PublicKey)
	return vectors.EncodeBase58(public)
}

// deployableConfiguration copies a committed configuration into a run-local
// chains root with its placeholders filled in - the owner, the manifest's
// attestor set and, on Solana, the program the pinned vectors derive from -
// and returns the copy's path, laid out as <root>/<chain>/config.json.
func deployableConfiguration(t *testing.T, chain string) string {
	t.Helper()
	raw, err := os.ReadFile(committedConfiguration(t, chain))
	if err != nil {
		t.Fatalf("reading the %s configuration: %v", chain, err)
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.UseNumber()
	document := map[string]any{}
	if err := decoder.Decode(&document); err != nil {
		t.Fatalf("decoding the %s configuration: %v", chain, err)
	}
	attestors := make([]string, 0, 5)
	for _, signer := range loadManifest(t, testManifest).Signers() {
		attestors = append(attestors, signer.Hex())
	}
	document["attestors"] = attestors
	if chain == "solana" {
		document["owner"] = solanaOwner()
		document["solana"].(map[string]any)["program_id"] = vectors.VectorProgramID.Base58()
	} else {
		document["owner"] = "0x2b7e151628aed2a6abf7158809cf4f3c762e7160"
	}
	encoded, err := json.MarshalIndent(document, "", "  ")
	if err != nil {
		t.Fatalf("encoding the %s configuration: %v", chain, err)
	}
	path := filepath.Join(t.TempDir(), chain, "config.json")
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatalf("creating the %s chains root: %v", chain, err)
	}
	if err := os.WriteFile(path, encoded, 0o644); err != nil {
		t.Fatalf("writing the %s configuration: %v", chain, err)
	}
	return path
}

func loadCanonical(t *testing.T, path string) *chainconfig.ChainConfig {
	t.Helper()
	cfg, err := chainconfig.Load(path)
	if err != nil {
		t.Fatalf("loading %s through chainconfig: %v", path, err)
	}
	return cfg
}

const ethereumVault = "0x7a3e5c81b04d296f8e1a7c35d92b6f04e8c1a37d"

func TestCommittedConfigurationsGenerateNothingWhileTheyCarryPlaceholders(t *testing.T) {
	for _, chain := range []string{"ethereum", "solana"} {
		t.Run(chain, func(t *testing.T) {
			cfg := loadCanonical(t, committedConfiguration(t, chain))
			_, err := FromChainConfig(cfg, governanceAuthority, vectors.VectorVaultHandle.Hex())
			if err == nil {
				t.Fatal("a committed configuration carrying placeholders was read as deployable")
			}
			if !strings.Contains(err.Error(), "placeholder") || !strings.Contains(err.Error(), "owner") {
				t.Fatalf("the refusal does not name the placeholder owner: %v", err)
			}
		})
	}
}

func TestChainconfigConfigurationsGenerateTheBodiesTheirValuesName(t *testing.T) {
	path := deployableConfiguration(t, "ethereum")
	canonical := loadCanonical(t, path)
	cfg, err := FromChainConfig(canonical, governanceAuthority, ethereumVault)
	if err != nil {
		t.Fatalf("a deployable configuration was refused: %v", err)
	}
	bundle, err := Generate(cfg, loadManifest(t, testManifest))
	if err != nil {
		t.Fatalf("generating from a deployable configuration: %v", err)
	}
	if bundle.Register.Chain.ChainID != canonical.ChainID || bundle.Register.Chain.FinalityDepth != canonical.FinalityDepth {
		t.Fatalf("the registration names chain %d at depth %d, the configuration %d at depth %d",
			bundle.Register.Chain.ChainID, bundle.Register.Chain.FinalityDepth, canonical.ChainID, canonical.FinalityDepth)
	}
	if got := bundle.Register.Chain.Vault.Hex(); got != ethereumVault {
		t.Fatalf("the registration names the vault %s, want %s", got, ethereumVault)
	}
	if len(bundle.Caps) != len(canonical.Assets) {
		t.Fatalf("%d caps for %d assets", len(bundle.Caps), len(canonical.Assets))
	}
	for i, asset := range canonical.Assets {
		capMsg := bundle.Caps[i]
		if capMsg.Asset.Hex() != strings.ToLower(asset.AssetID) {
			t.Fatalf("cap %d is for %s, the configuration lists %s", i, capMsg.Asset.Hex(), asset.AssetID)
		}
		if capMsg.MaxPerTx.String() != asset.PerTxCap || capMsg.MaxInFlight.String() != asset.TotalCap {
			t.Fatalf("cap %d is %s per transaction and %s in total, the configuration names %s and %s",
				i, capMsg.MaxPerTx, capMsg.MaxInFlight, asset.PerTxCap, asset.TotalCap)
		}
	}
	if bundle.Caps[0].Asset != (types.Address20{}) {
		t.Fatalf("the first cap is for %s, not the native coin", bundle.Caps[0].Asset.Hex())
	}
	zeroVault, err := FromChainConfig(canonical, governanceAuthority, "0x"+strings.Repeat("00", 20))
	if err == nil {
		_, err = Generate(zeroVault, loadManifest(t, testManifest))
	}
	if err == nil {
		t.Fatal("a zero vault was accepted")
	}
	if refusal := fieldError(t, err); refusal.Field != fieldVault || refusal.File != path {
		t.Fatalf("the zero vault refusal names %s in %s", refusal.Field, refusal.File)
	}
}

func TestSolanaVaultIsTheHandleOfTheVaultAuthority(t *testing.T) {
	path := deployableConfiguration(t, "solana")
	canonical := loadCanonical(t, path)
	cfg, err := FromChainConfig(canonical, governanceAuthority, vectors.VectorVaultHandle.Hex())
	if err != nil {
		t.Fatalf("the vault-authority handle was refused: %v", err)
	}
	if cfg.Vault != vectors.VectorVaultHandle.Hex() || cfg.ProgramID != vectors.VectorProgramID.Base58() {
		t.Fatalf("the generator reads the vault %s of program %s", cfg.Vault, cfg.ProgramID)
	}
	other := vectors.Handle(vectors.VectorProgramID).Hex()
	_, err = FromChainConfig(canonical, governanceAuthority, other)
	if err == nil {
		t.Fatal("a vault that is not the vault-authority handle was accepted")
	}
	refusal := fieldError(t, err)
	if refusal.Field != fieldVault || !strings.Contains(refusal.Msg, "vault-authority PDA") {
		t.Fatalf("the refusal names %s: %s", refusal.Field, refusal.Msg)
	}
}

func TestSolanaReadbackNamesTheProgramsAccountsAndSidiorasDenom(t *testing.T) {
	canonical := loadCanonical(t, deployableConfiguration(t, "solana"))
	cfg, err := FromChainConfig(canonical, governanceAuthority, vectors.VectorVaultHandle.Hex())
	if err != nil {
		t.Fatalf("reading the deployable Solana configuration: %v", err)
	}
	bundle, err := Generate(cfg, loadManifest(t, testManifest))
	if err != nil {
		t.Fatalf("generating the Solana bundle: %v", err)
	}
	readback, err := ReadbackOf(canonical, bundle)
	if err != nil {
		t.Fatalf("deriving the Solana read-back: %v", err)
	}
	if readback.ChainID != SolanaChainID || readback.Vault != vectors.VectorVaultHandle.Hex() {
		t.Fatalf("the read-back names chain %d and vault %s", readback.ChainID, readback.Vault)
	}
	owner, err := vectors.Key(solanaOwner())
	if err != nil {
		t.Fatalf("the owner key does not decode: %v", err)
	}
	if want := "0x" + hex.EncodeToString(owner[:]); readback.Owner != want {
		t.Fatalf("the owner reads back as %s, want %s", readback.Owner, want)
	}
	configAccount, _, err := vectors.FindProgramAddress(vectors.VectorProgramID, [][]byte{[]byte("config")})
	if err != nil {
		t.Fatalf("deriving the config account: %v", err)
	}
	if readback.Solana == nil || readback.Solana.ConfigAccount != configAccount.Base58() ||
		readback.Solana.VaultAuthority != vectors.VectorVaultAuthority.Base58() {
		t.Fatalf("the read-back names the accounts %+v", readback.Solana)
	}
	if len(readback.Assets) != 2 {
		t.Fatalf("the read-back carries %d assets, want 2", len(readback.Assets))
	}
	native, sidiora := readback.Assets[0], readback.Assets[1]
	if native.AssetID != vectors.WrappedSolAssetID.Hex() || native.Denom != types.Denom(SolanaChainID, types.Address20(vectors.WrappedSolAssetID)) {
		t.Fatalf("the native coin reads back as %s minted as %s", native.AssetID, native.Denom)
	}
	if sidiora.AssetID != SidioraAssetID().Hex() || sidiora.Denom != types.SidioraDenom() {
		t.Fatalf("Sidiora reads back as %s minted as %s, want %s minted as %s",
			sidiora.AssetID, sidiora.Denom, SidioraAssetID().Hex(), types.SidioraDenom())
	}
	sidioraAccount, _, err := vectors.FindProgramAddress(vectors.VectorProgramID, [][]byte{[]byte("asset"), vectors.SidioraMint[:]})
	if err != nil {
		t.Fatalf("deriving Sidiora's asset account: %v", err)
	}
	if sidiora.Account != sidioraAccount.Base58() || sidiora.Mint != "0x"+hex.EncodeToString(vectors.SidioraMint[:]) {
		t.Fatalf("Sidiora's asset account reads back as %s of mint %s", sidiora.Account, sidiora.Mint)
	}
	for i, asset := range readback.Assets {
		if asset.PerTxCap != bundle.Caps[i].MaxPerTx.String() || asset.TotalCap != bundle.Caps[i].MaxInFlight.String() {
			t.Fatalf("asset %d reads back caps other than the body sets", i)
		}
	}
}

func TestRunWritesTheReadbackBesideTheBodies(t *testing.T) {
	config := deployableConfiguration(t, "solana")
	parent := t.TempDir()
	out := filepath.Join(parent, "bodies")
	readbackPath := filepath.Join(parent, "readback.json")
	var report bytes.Buffer
	args := []string{"-manifest", testManifest, "-authority", governanceAuthority,
		"-vault", vectors.VectorVaultHandle.Hex(), "-readback", readbackPath, config, out}
	if err := Run(args, &report); err != nil {
		t.Fatalf("the command refused a deployable chainconfig configuration: %v", err)
	}
	entries, err := os.ReadDir(out)
	if err != nil || len(entries) != 4 {
		t.Fatalf("the command wrote %d bodies (%v), want 4", len(entries), err)
	}
	raw, err := os.ReadFile(readbackPath)
	if err != nil {
		t.Fatalf("reading the read-back expectation: %v", err)
	}
	readback := decodeJSONStrict[Readback](t, readbackPath, raw)
	if readback.Chain != "solana" || len(readback.Assets) != 2 || readback.Assets[1].Denom != types.SidioraDenom() {
		t.Fatalf("the read-back expectation on disk is %+v", readback)
	}
	if !strings.Contains(report.String(), readbackPath) {
		t.Fatalf("the command did not report %s", readbackPath)
	}

	again := filepath.Join(parent, "again")
	err = Run([]string{"-manifest", testManifest, "-authority", governanceAuthority,
		"-vault", vectors.VectorVaultHandle.Hex(), "-readback", readbackPath, config, again}, &report)
	if err == nil {
		t.Fatal("the command overwrote an existing read-back expectation")
	}
	if _, statErr := os.Stat(again); !os.IsNotExist(statErr) {
		t.Fatalf("the refused run left %s behind (%v)", again, statErr)
	}

	for _, partial := range [][]string{
		{"-authority", governanceAuthority},
		{"-vault", vectors.VectorVaultHandle.Hex()},
		{"-readback", filepath.Join(parent, "partial.json")},
	} {
		target := filepath.Join(parent, "partial")
		args := append(append([]string{"-manifest", testManifest}, partial...), config, target)
		if err := Run(args, &report); err == nil {
			t.Fatalf("the command ran with only %v", partial)
		}
		if _, statErr := os.Stat(target); !os.IsNotExist(statErr) {
			t.Fatalf("the refused run with only %v left %s behind", partial, target)
		}
	}
}

const bridgeProposalTypeURL = "/paxprotocol.paxchain.layerxbridge.BridgeProposal"

// proposalEnvelope is the top level of an emitted proposal: the content's type
// URL, its title and description, and every carried message as an Any.
type proposalEnvelope struct {
	Type        string            `json:"@type"`
	Title       string            `json:"title"`
	Description string            `json:"description"`
	Messages    []json.RawMessage `json:"messages"`
}

// decodeProposalStrict decodes one emitted proposal back into the module's
// proposal content and its messages, and requires every carried message to
// read back through DecodeBody as the body of the same message would.
func decodeProposalStrict(t *testing.T, file File) []sdk.Msg {
	t.Helper()
	envelope := decodeJSONStrict[proposalEnvelope](t, file.Name, file.Body)
	if envelope.Type != bridgeProposalTypeURL {
		t.Fatalf("%s carries the type URL %q, want %q", file.Name, envelope.Type, bridgeProposalTypeURL)
	}
	if strings.TrimSpace(envelope.Title) == "" || strings.TrimSpace(envelope.Description) == "" {
		t.Fatalf("%s carries no title or no description", file.Name)
	}
	proposal, err := DecodeProposal(file.Body)
	if err != nil {
		t.Fatalf("decoding %s back into the proposal content: %v\n%s", file.Name, err, file.Body)
	}
	if proposal.Title != envelope.Title || proposal.Description != envelope.Description {
		t.Fatalf("%s decodes to another title or description", file.Name)
	}
	msgs, err := proposal.GetMessages()
	if err != nil {
		t.Fatalf("%s: %v", file.Name, err)
	}
	if len(msgs) != len(envelope.Messages) {
		t.Fatalf("%s carries %d messages and decodes to %d", file.Name, len(envelope.Messages), len(msgs))
	}
	for i, raw := range envelope.Messages {
		body, err := DecodeBody(raw)
		if err != nil {
			t.Fatalf("%s message %d does not read back as a body: %v", file.Name, i, err)
		}
		if sdk.MsgTypeURL(body) != sdk.MsgTypeURL(msgs[i]) {
			t.Fatalf("%s message %d is %s as a body and %s in the proposal", file.Name, i, sdk.MsgTypeURL(body), sdk.MsgTypeURL(msgs[i]))
		}
	}
	return msgs
}

func proposalFiles(t *testing.T, bundle Bundle) []File {
	t.Helper()
	files, err := bundle.ProposalFiles()
	if err != nil {
		t.Fatalf("marshalling the proposals: %v", err)
	}
	return files
}

func sameCarried(t *testing.T, name string, got sdk.Msg, want codec.ProtoMarshaler) {
	t.Helper()
	carried, ok := got.(codec.ProtoMarshaler)
	if !ok {
		t.Fatalf("%s carries a %T", name, got)
	}
	sameMessage(t, name, want, carried)
}

func TestEthereumProposalCarriesEveryBodyFieldForField(t *testing.T) {
	bundle := generate(t, ethereumPath)
	files := proposalFiles(t, bundle)
	if len(files) != 1 || files[0].Name != OpenChainProposalFile {
		t.Fatalf("ethereum emitted %d proposals, want only %s", len(files), OpenChainProposalFile)
	}
	msgs := decodeProposalStrict(t, files[0])
	if len(msgs) != 2+len(bundle.Caps) {
		t.Fatalf("the proposal carries %d messages for %d caps", len(msgs), len(bundle.Caps))
	}
	sameCarried(t, "the registration", msgs[0], &bundle.Register)
	sameCarried(t, "the attestor set", msgs[1], &bundle.Attestors)
	for i := range bundle.Caps {
		sameCarried(t, fmt.Sprintf("cap %d", i), msgs[2+i], &bundle.Caps[i])
	}
	if first := msgs[2].(*types.MsgSetCap); first.Asset != (types.Address20{}) {
		t.Fatalf("the first cap the proposal carries is for %s, want the native coin", first.Asset.Hex())
	}
	for i, msg := range msgs {
		if signers := msg.GetSigners(); len(signers) != 1 || signers[0].String() != governanceAuthority {
			t.Fatalf("message %d is signed by %v, want the governance module account", i, signers)
		}
	}
}

func TestSolanaProposalsSetSidiorasCapApart(t *testing.T) {
	bundle := generate(t, solanaPath)
	files := proposalFiles(t, bundle)
	if len(files) != 2 || files[0].Name != OpenChainProposalFile || files[1].Name != SidioraCapProposalFile {
		t.Fatalf("solana emitted %d proposals, want %s and %s", len(files), OpenChainProposalFile, SidioraCapProposalFile)
	}
	open := decodeProposalStrict(t, files[0])
	if len(open) != 3 {
		t.Fatalf("the opening proposal carries %d messages, want the registration, the attestor set and the wrapped SOL cap", len(open))
	}
	sameCarried(t, "the registration", open[0], &bundle.Register)
	sameCarried(t, "the attestor set", open[1], &bundle.Attestors)
	sameCarried(t, "the wrapped SOL cap", open[2], &bundle.Caps[0])
	for i, msg := range open {
		if capMsg, ok := msg.(*types.MsgSetCap); ok && capMsg.Asset == SidioraAssetID() {
			t.Fatalf("the opening proposal carries Sidiora's cap as message %d", i)
		}
	}
	sidiora := decodeProposalStrict(t, files[1])
	if len(sidiora) != 1 {
		t.Fatalf("the Sidiora proposal carries %d messages, want its cap alone", len(sidiora))
	}
	sameCarried(t, "Sidiora's cap", sidiora[0], &bundle.Caps[1])
	if !strings.Contains(string(files[1].Body), types.SidioraDenom()) {
		t.Fatalf("the Sidiora proposal does not name the usid denom %s it waits for", types.SidioraDenom())
	}
}

func TestProposalsTheHandlerWouldRefuseAreRefused(t *testing.T) {
	files := proposalFiles(t, generate(t, ethereumPath))
	body := files[0].Body
	bridgeAccount := types.ModuleAddress().String()
	for _, testCase := range []struct {
		name string
		from string
		to   string
	}{
		{"an unknown proposal field", `"title":`, `"surplus": true, "title":`},
		{"an unknown message field", `"chain_id":`, `"surplus": true, "chain_id":`},
		{"another authority", `"authority": "` + governanceAuthority + `"`, `"authority": "` + bridgeAccount + `"`},
		{"a message of another module", `"@type": "/paxprotocol.paxchain.layerxbridge.MsgSetAttestors"`, `"@type": "/cosmos.gov.v1beta1.TextProposal"`},
		{"another proposal type", `"@type": "` + bridgeProposalTypeURL + `"`, `"@type": "/cosmos.gov.v1beta1.TextProposal"`},
		{"an empty title", `"title": "Open ethereum on the Paxeer X Network bridge"`, `"title": ""`},
	} {
		t.Run(testCase.name, func(t *testing.T) {
			if !bytes.Contains(body, []byte(testCase.from)) {
				t.Fatalf("the proposal carries no %s\n%s", testCase.from, body)
			}
			altered := bytes.Replace(body, []byte(testCase.from), []byte(testCase.to), 1)
			if _, err := DecodeProposal(altered); err == nil {
				t.Fatalf("a proposal with %s was decoded\n%s", testCase.name, altered)
			}
		})
	}
}

func TestRunWritesTheProposalsApartFromTheBodies(t *testing.T) {
	root := t.TempDir()
	out := filepath.Join(root, "solana")
	proposalsDir := filepath.Join(root, "solana-proposals")
	var report bytes.Buffer
	if err := Run([]string{"-manifest", testManifest, "-proposals", proposalsDir, solanaPath, out}, &report); err != nil {
		t.Fatalf("the command refused a configuration it should accept: %v", err)
	}
	bundle := generate(t, solanaPath)
	bodies, err := bundle.Files()
	if err != nil {
		t.Fatalf("marshalling the bundle: %v", err)
	}
	for dir, want := range map[string][]File{out: bodies, proposalsDir: proposalFiles(t, bundle)} {
		entries, err := os.ReadDir(dir)
		if err != nil {
			t.Fatalf("reading %s: %v", dir, err)
		}
		if len(entries) != len(want) {
			t.Fatalf("the command wrote %d files into %s, want %d", len(entries), dir, len(want))
		}
		for _, file := range want {
			path := filepath.Join(dir, file.Name)
			written, err := os.ReadFile(path)
			if err != nil {
				t.Fatalf("reading %s: %v", path, err)
			}
			if !bytes.Equal(written, file.Body) {
				t.Fatalf("%s on disk is not what the generator built", path)
			}
			if !strings.Contains(report.String(), path) {
				t.Fatalf("the command did not report %s", path)
			}
		}
	}
}

func TestRunWritesNoProposalsOrBodiesWhenTheProposalsCannotBeWritten(t *testing.T) {
	root := t.TempDir()
	out := filepath.Join(root, "solana")
	var report bytes.Buffer
	if err := Run([]string{"-manifest", testManifest, "-proposals", out, solanaPath, out}, &report); err == nil {
		t.Fatal("the command wrote the proposals into the bodies' directory")
	}
	if _, err := os.Stat(out); !os.IsNotExist(err) {
		t.Fatalf("the refused run left %s behind (%v)", out, err)
	}

	proposalsDir := filepath.Join(root, "proposals")
	if err := os.Mkdir(proposalsDir, 0o755); err != nil {
		t.Fatalf("creating the proposals directory: %v", err)
	}
	if err := os.WriteFile(filepath.Join(proposalsDir, OpenChainProposalFile), []byte("{}\n"), 0o644); err != nil {
		t.Fatalf("writing a stale proposal: %v", err)
	}
	if err := Run([]string{"-manifest", testManifest, "-proposals", proposalsDir, solanaPath, out}, &report); err == nil {
		t.Fatal("the command wrote proposals beside a proposal of an earlier bundle")
	}
	if _, err := os.Stat(out); !os.IsNotExist(err) {
		t.Fatalf("the refused run wrote the bodies to %s (%v)", out, err)
	}
	entries, err := os.ReadDir(proposalsDir)
	if err != nil {
		t.Fatalf("reading the proposals directory: %v", err)
	}
	if len(entries) != 1 {
		t.Fatalf("the refused run changed the proposals directory: %d entries", len(entries))
	}

	err = Run([]string{"-manifest", testManifest, "-proposals", filepath.Join(root, "refused"),
		filepath.Join("testdata", "refuse", "non-governance-authority.json"), filepath.Join(root, "refused-bodies")}, &report)
	if refusal := fieldError(t, err); refusal.Field != fieldAuthority {
		t.Fatalf("the refusal names the field %q, want %q", refusal.Field, fieldAuthority)
	}
	for _, name := range []string{"refused", "refused-bodies"} {
		if _, err := os.Stat(filepath.Join(root, name)); !os.IsNotExist(err) {
			t.Fatalf("the refused run left %s behind (%v)", name, err)
		}
	}
}
