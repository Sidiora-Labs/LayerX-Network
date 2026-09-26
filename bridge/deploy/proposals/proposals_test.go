package proposals

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sidiora-labs/paxeer-network/modules/layerxbridge/types"
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

// decodeStrict decodes one emitted body back into the module's own message
// type with unknown fields forbidden, so a body that carries a field the
// keeper does not know fails the test rather than reaching governance.
func decodeStrict[T any](t *testing.T, name string, body []byte) T {
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
