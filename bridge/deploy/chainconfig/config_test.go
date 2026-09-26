package chainconfig

import (
	"bytes"
	"encoding/binary"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sidiora-labs/paxeer-network/bridge/vectors"
)

// repositoryRoot is where the nine committed configurations live, relative to
// this package.
func repositoryRoot(t *testing.T) string {
	t.Helper()
	root, err := filepath.Abs(filepath.Join("..", "..", ".."))
	if err != nil {
		t.Fatalf("the repository root is not reachable: %v", err)
	}
	return root
}

// document reads a committed configuration as JSON, keeping every number as it
// was written so a re-encoded copy still carries the Solana chain id exactly.
func document(t *testing.T, root, chain string) map[string]any {
	t.Helper()
	path, err := Path(root, chain)
	if err != nil {
		t.Fatalf("%s has no configuration path: %v", chain, err)
	}
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("%s is not readable: %v", path, err)
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.UseNumber()
	decoded := map[string]any{}
	if err := decoder.Decode(&decoded); err != nil {
		t.Fatalf("%s is not a JSON object: %v", path, err)
	}
	return decoded
}

// copyOf writes a committed configuration into a temporary chains root after
// applying one change, so every refusal below differs from an accepted
// configuration in exactly that field.
func copyOf(t *testing.T, root, chain string, change func(map[string]any)) string {
	t.Helper()
	decoded := document(t, root, chain)
	if change != nil {
		change(decoded)
	}
	directory := filepath.Join(t.TempDir(), chain)
	if err := os.MkdirAll(directory, 0o750); err != nil {
		t.Fatalf("the temporary chain directory was not created: %v", err)
	}
	encoded, err := json.MarshalIndent(decoded, "", "  ")
	if err != nil {
		t.Fatalf("the changed configuration was not encoded: %v", err)
	}
	path := filepath.Join(directory, "config.json")
	if err := os.WriteFile(path, append(encoded, '\n'), 0o600); err != nil {
		t.Fatalf("the changed configuration was not written: %v", err)
	}
	return path
}

func assets(t *testing.T, decoded map[string]any) []any {
	t.Helper()
	list, ok := decoded["assets"].([]any)
	if !ok {
		t.Fatalf("the configuration carries no asset list")
	}
	return list
}

func asset(t *testing.T, decoded map[string]any, index int) map[string]any {
	t.Helper()
	list := assets(t, decoded)
	if index >= len(list) {
		t.Fatalf("the configuration carries %d assets, not %d", len(list), index+1)
	}
	entry, ok := list[index].(map[string]any)
	if !ok {
		t.Fatalf("asset %d is not an object", index)
	}
	return entry
}

func section(t *testing.T, decoded map[string]any, name string) map[string]any {
	t.Helper()
	value, ok := decoded[name].(map[string]any)
	if !ok {
		t.Fatalf("the configuration carries no %s section", name)
	}
	return value
}

// realAttestors is a filled attestor set: five distinct, non-zero addresses in
// the strictly ascending order every destination verifies.
func realAttestors() []any {
	return []any{
		"0x1111111111111111111111111111111111111111",
		"0x2222222222222222222222222222222222222222",
		"0x3333333333333333333333333333333333333333",
		"0x4444444444444444444444444444444444444444",
		"0x5555555555555555555555555555555555555555",
	}
}

const realOwner = "0x6666666666666666666666666666666666666666"

// secondEVMAsset is a registered ERC20 beside the native coin, used by the
// refusals that need an asset the chain does not fix.
func secondEVMAsset() map[string]any {
	return map[string]any{
		"symbol":     "USDC",
		"address":    "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
		"asset_id":   "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
		"decimals":   json.Number("6"),
		"per_tx_cap": "500000000000",
		"total_cap":  "10000000000000",
	}
}

func TestCommittedConfigurationsAreAccepted(t *testing.T) {
	root := repositoryRoot(t)
	if len(Chains()) != 9 {
		t.Fatalf("the bridge carries nine chains, not %d", len(Chains()))
	}
	for _, chain := range Chains() {
		path, err := Path(root, chain.Name)
		if err != nil {
			t.Fatalf("%s has no configuration path: %v", chain.Name, err)
		}
		config, err := Load(path)
		if err != nil {
			t.Fatalf("the committed configuration of %s is refused: %v", chain.Name, err)
		}
		if config.Source() != path {
			t.Errorf("%s: the configuration names %q as its source", chain.Name, config.Source())
		}
		if config.ChainID != chain.ID || config.Kind != chain.Kind {
			t.Errorf("%s: the configuration claims chain %d of kind %s", chain.Name, config.ChainID, config.Kind)
		}
		if config.Native.Symbol != chain.NativeSymbol || config.Native.Decimals != chain.NativeDecimals {
			t.Errorf("%s: the configuration claims %s with %d decimals", chain.Name, config.Native.Symbol, config.Native.Decimals)
		}
		if config.FinalityDepth == 0 || config.Threshold == 0 || int(config.Threshold) > len(config.Attestors) {
			t.Errorf("%s: threshold %d of %d attestors at finality depth %d", chain.Name, config.Threshold, len(config.Attestors), config.FinalityDepth)
		}
		native := config.NativeAsset()
		if native.Symbol != chain.NativeSymbol || native.Decimals != chain.NativeDecimals {
			t.Errorf("%s: the first asset is %s with %d decimals", chain.Name, native.Symbol, native.Decimals)
		}
		perTx, err := native.PerTx()
		if err != nil {
			t.Fatalf("%s: the native per-transaction cap is unreadable: %v", chain.Name, err)
		}
		total, err := native.Total()
		if err != nil {
			t.Fatalf("%s: the native total cap is unreadable: %v", chain.Name, err)
		}
		if perTx.Sign() == 0 || total.Sign() == 0 {
			t.Errorf("%s: the native coin is capped at %s per transaction and %s in total", chain.Name, native.PerTxCap, native.TotalCap)
		}
		switch chain.Kind {
		case KindEVM:
			if !strings.EqualFold(native.Address, NativeEVMAsset) || !strings.EqualFold(native.AssetID, NativeEVMAsset) {
				t.Errorf("%s: the native asset is %s with id %s", chain.Name, native.Address, native.AssetID)
			}
			if config.Environment.ExplorerKey == "" || config.Solana != nil {
				t.Errorf("%s: an EVM chain verifies through an explorer key and holds no program", chain.Name)
			}
		case KindSolana:
			if native.Address != WrappedSolMint {
				t.Errorf("%s: the native asset is the mint %s", chain.Name, native.Address)
			}
			if config.Solana == nil || config.Solana.Commitment != "finalized" {
				t.Errorf("%s: the Solana section is %+v", chain.Name, config.Solana)
			}
		}
		if (chain.Name == BigBlockChain) != (config.BigBlocks != nil) {
			t.Errorf("%s: big_blocks is recorded as %+v", chain.Name, config.BigBlocks)
		}
	}
}

func TestCommittedSolanaAssetIdentities(t *testing.T) {
	root := repositoryRoot(t)
	path, err := Path(root, "solana")
	if err != nil {
		t.Fatalf("solana has no configuration path: %v", err)
	}
	config, err := Load(path)
	if err != nil {
		t.Fatalf("the committed Solana configuration is refused: %v", err)
	}
	if config.ChainID != SolanaChainID {
		t.Fatalf("Solana's chain id is %d, not %d", SolanaChainID, config.ChainID)
	}
	if len(config.Assets) != 2 {
		t.Fatalf("the Solana configuration registers %d assets, not the wrapped SOL mint and Sidiora's", len(config.Assets))
	}
	handle, err := SolanaHandle(WrappedSolMint)
	if err != nil {
		t.Fatalf("the wrapped SOL handle is not derivable: %v", err)
	}
	if !strings.EqualFold(config.Assets[0].AssetID, handle) {
		t.Errorf("the wrapped SOL asset id is %s, not its derived handle %s", config.Assets[0].AssetID, handle)
	}
	if config.Assets[1].Address != SidioraMint {
		t.Errorf("the second Solana asset is %s, not Sidiora's mint", config.Assets[1].Address)
	}
	if config.Assets[1].AssetID != SidioraAssetID {
		t.Errorf("Sidiora's asset id is %s, not %s", SidioraAssetID, config.Assets[1].AssetID)
	}
	if config.Assets[1].Decimals != 6 {
		t.Errorf("Sidiora carries six decimals, not %d", config.Assets[1].Decimals)
	}
	sidioraHandle, err := SolanaHandle(SidioraMint)
	if err != nil {
		t.Fatalf("Sidiora's mint handle is not derivable: %v", err)
	}
	if strings.EqualFold(sidioraHandle, SidioraAssetID) {
		t.Errorf("the registry records Sidiora's fixed id because it differs from the derived handle %s", sidioraHandle)
	}
}

func TestSolanaChainIDIsTheReservedValue(t *testing.T) {
	padded := make([]byte, 8)
	copy(padded[2:], "SOLANA")
	if reserved := binary.BigEndian.Uint64(padded); reserved != SolanaChainID {
		t.Fatalf("the padded ASCII bytes of SOLANA read %d, not %d", reserved, SolanaChainID)
	}
	for _, chain := range Chains() {
		if chain.Kind == KindEVM && chain.ID >= SolanaChainID {
			t.Errorf("%s carries chain id %d, which collides with Solana's reserved id", chain.Name, chain.ID)
		}
	}
}

func TestSolanaHandleDerivation(t *testing.T) {
	cases := map[string]string{
		WrappedSolMint: "0xcf996523b5d068a26f0aa8a116602fe5033ee3a1",
		SidioraMint:    "0x9232467d43fd9edd0bf250fe24ca0c9380b3ca86",
	}
	for key, want := range cases {
		got, err := SolanaHandle(key)
		if err != nil {
			t.Fatalf("%s has no handle: %v", key, err)
		}
		if !strings.EqualFold(got, want) {
			t.Errorf("the handle of %s is %s, not %s", key, want, got)
		}
	}
	if _, err := SolanaHandle("So1111111111111111111111111111111111111111I"); err == nil {
		t.Error("a key carrying a character outside the base58 alphabet is accepted")
	}
	if _, err := SolanaHandle("11111111111111111111111111111111111111111111111111111111111111111"); err == nil {
		t.Error("a key that is not 32 bytes is accepted")
	}
	if _, err := DecodeBase58(""); err == nil {
		t.Error("the empty string is accepted as a base58 key")
	}
	raw, err := DecodeBase58(SolanaZeroPubkey)
	if err != nil {
		t.Fatalf("the zero pubkey is not decodable: %v", err)
	}
	if !bytes.Equal(raw, make([]byte, 32)) {
		t.Errorf("the zero pubkey decodes to %x", raw)
	}
}

func TestCommittedConfigurationsAreNotDeployable(t *testing.T) {
	root := repositoryRoot(t)
	for _, chain := range Chains() {
		path, err := Path(root, chain.Name)
		if err != nil {
			t.Fatalf("%s has no configuration path: %v", chain.Name, err)
		}
		config, err := Load(path)
		if err != nil {
			t.Fatalf("the committed configuration of %s is refused: %v", chain.Name, err)
		}
		err = config.RequireDeployable()
		if err == nil {
			t.Fatalf("%s: a configuration carrying placeholders is deployable", chain.Name)
		}
		if !strings.Contains(err.Error(), path) || !strings.Contains(err.Error(), "owner") {
			t.Errorf("%s: the refusal does not name the file and the field: %v", chain.Name, err)
		}
	}
}

func TestRequireDeployableRefusesEachPlaceholder(t *testing.T) {
	root := repositoryRoot(t)
	cases := []struct {
		name   string
		chain  string
		change func(map[string]any)
		field  string
	}{
		{
			name:  "the placeholder owner",
			chain: "ethereum",
			change: func(decoded map[string]any) {
				decoded["attestors"] = realAttestors()
			},
			field: "owner",
		},
		{
			name:  "a placeholder attestor",
			chain: "ethereum",
			change: func(decoded map[string]any) {
				decoded["owner"] = realOwner
			},
			field: "attestors[0]",
		},
		{
			name:  "the placeholder program id",
			chain: "solana",
			change: func(decoded map[string]any) {
				decoded["owner"] = WrappedSolMint
				decoded["attestors"] = realAttestors()
			},
			field: "solana.program_id",
		},
		{
			name:  "the unacknowledged big-block requirement",
			chain: "hyperevm",
			change: func(decoded map[string]any) {
				decoded["owner"] = realOwner
				decoded["attestors"] = realAttestors()
			},
			field: "big_blocks.acknowledged",
		},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			path := copyOf(t, root, test.chain, test.change)
			config, err := Load(path)
			if err != nil {
				t.Fatalf("the changed configuration is refused by Validate: %v", err)
			}
			err = config.RequireDeployable()
			if err == nil {
				t.Fatalf("%s is deployable", test.field)
			}
			if !strings.Contains(err.Error(), path) || !strings.Contains(err.Error(), test.field) {
				t.Errorf("the refusal does not name the file and %s: %v", test.field, err)
			}
		})
	}
}

func TestFilledConfigurationIsDeployable(t *testing.T) {
	root := repositoryRoot(t)
	for _, chain := range []string{"ethereum", "hyperevm", "solana"} {
		t.Run(chain, func(t *testing.T) {
			path := copyOf(t, root, chain, func(decoded map[string]any) {
				decoded["attestors"] = realAttestors()
				if chain == "solana" {
					decoded["owner"] = WrappedSolMint
					section(t, decoded, "solana")["program_id"] = SidioraMint
				} else {
					decoded["owner"] = realOwner
				}
				if chain == BigBlockChain {
					section(t, decoded, "big_blocks")["acknowledged"] = true
				}
			})
			config, err := Load(path)
			if err != nil {
				t.Fatalf("a filled configuration is refused: %v", err)
			}
			if err := config.RequireDeployable(); err != nil {
				t.Fatalf("a filled configuration is not deployable: %v", err)
			}
		})
	}
}

func TestCopyOfACommittedConfigurationIsAccepted(t *testing.T) {
	root := repositoryRoot(t)
	for _, chain := range Chains() {
		path := copyOf(t, root, chain.Name, nil)
		if _, err := Load(path); err != nil {
			t.Fatalf("an unchanged copy of %s is refused: %v", chain.Name, err)
		}
	}
}

func TestRefusals(t *testing.T) {
	root := repositoryRoot(t)
	cases := []struct {
		name   string
		chain  string
		change func(*testing.T, map[string]any)
		field  string
	}{
		{
			name:  "an unknown field",
			chain: "ethereum",
			change: func(_ *testing.T, decoded map[string]any) {
				decoded["rpc_url"] = "https://example.invalid"
			},
			field: "rpc_url",
		},
		{
			name:  "an unknown chain",
			chain: "ethereum",
			change: func(_ *testing.T, decoded map[string]any) {
				decoded["chain"] = "sepolia"
			},
			field: "chain",
		},
		{
			name:  "a chain id that disagrees with the chain",
			chain: "ethereum",
			change: func(_ *testing.T, decoded map[string]any) {
				decoded["chain_id"] = json.Number("2")
			},
			field: "chain_id",
		},
		{
			name:  "a kind that disagrees with the chain",
			chain: "ethereum",
			change: func(_ *testing.T, decoded map[string]any) {
				decoded["kind"] = string(KindSolana)
			},
			field: "kind",
		},
		{
			name:  "a native symbol that disagrees with the chain",
			chain: "ethereum",
			change: func(_ *testing.T, decoded map[string]any) {
				decoded["native"].(map[string]any)["symbol"] = "WETH"
			},
			field: "native.symbol",
		},
		{
			name:  "native decimals that disagree with the chain",
			chain: "ethereum",
			change: func(_ *testing.T, decoded map[string]any) {
				decoded["native"].(map[string]any)["decimals"] = json.Number("6")
			},
			field: "native.decimals",
		},
		{
			name:  "an empty environment variable name",
			chain: "ethereum",
			change: func(t *testing.T, decoded map[string]any) {
				section(t, decoded, "environment")["rpc_url"] = ""
			},
			field: "environment.rpc_url",
		},
		{
			name:  "an environment variable name that is not upper snake case",
			chain: "ethereum",
			change: func(t *testing.T, decoded map[string]any) {
				section(t, decoded, "environment")["deploy_key"] = "paxeer-bridge-key"
			},
			field: "environment.deploy_key",
		},
		{
			name:  "a missing explorer key variable on an EVM chain",
			chain: "ethereum",
			change: func(t *testing.T, decoded map[string]any) {
				delete(section(t, decoded, "environment"), "explorer_key")
			},
			field: "environment.explorer_key",
		},
		{
			name:  "a toolchain directory on an EVM chain",
			chain: "ethereum",
			change: func(t *testing.T, decoded map[string]any) {
				section(t, decoded, "environment")["toolchain_bin"] = "PAXEER_BRIDGE_ETHEREUM_TOOLCHAIN_BIN"
			},
			field: "environment.toolchain_bin",
		},
		{
			name:  "an explorer key variable on Solana",
			chain: "solana",
			change: func(t *testing.T, decoded map[string]any) {
				section(t, decoded, "environment")["explorer_key"] = "PAXEER_BRIDGE_SOLANA_EXPLORER_KEY"
			},
			field: "environment.explorer_key",
		},
		{
			name:  "a missing toolchain directory on Solana",
			chain: "solana",
			change: func(t *testing.T, decoded map[string]any) {
				delete(section(t, decoded, "environment"), "toolchain_bin")
			},
			field: "environment.toolchain_bin",
		},
		{
			name:  "a zero owner",
			chain: "ethereum",
			change: func(_ *testing.T, decoded map[string]any) {
				decoded["owner"] = NativeEVMAsset
			},
			field: "owner",
		},
		{
			name:  "a malformed owner",
			chain: "ethereum",
			change: func(_ *testing.T, decoded map[string]any) {
				decoded["owner"] = "0x6666"
			},
			field: "owner",
		},
		{
			name:  "a malformed placeholder owner",
			chain: "ethereum",
			change: func(_ *testing.T, decoded map[string]any) {
				decoded["owner"] = PlaceholderPrefix + "Owner Address"
			},
			field: "owner",
		},
		{
			name:  "a zero owner on Solana",
			chain: "solana",
			change: func(_ *testing.T, decoded map[string]any) {
				decoded["owner"] = SolanaZeroPubkey
			},
			field: "owner",
		},
		{
			name:  "an empty attestor set",
			chain: "ethereum",
			change: func(_ *testing.T, decoded map[string]any) {
				decoded["attestors"] = []any{}
			},
			field: "attestors",
		},
		{
			name:  "a half-filled attestor set",
			chain: "ethereum",
			change: func(_ *testing.T, decoded map[string]any) {
				set := realAttestors()
				set[2] = PlaceholderPrefix + "attestor-3"
				decoded["attestors"] = set
			},
			field: "attestors",
		},
		{
			name:  "a zero attestor",
			chain: "ethereum",
			change: func(_ *testing.T, decoded map[string]any) {
				set := realAttestors()
				set[0] = NativeEVMAsset
				decoded["attestors"] = set
			},
			field: "attestors[0]",
		},
		{
			name:  "a duplicated attestor",
			chain: "ethereum",
			change: func(_ *testing.T, decoded map[string]any) {
				set := realAttestors()
				set[1] = set[0]
				decoded["attestors"] = set
			},
			field: "attestors[1]",
		},
		{
			name:  "a descending attestor set",
			chain: "ethereum",
			change: func(_ *testing.T, decoded map[string]any) {
				set := realAttestors()
				set[0], set[1] = set[1], set[0]
				decoded["attestors"] = set
			},
			field: "attestors[1]",
		},
		{
			name:  "a threshold of zero",
			chain: "ethereum",
			change: func(_ *testing.T, decoded map[string]any) {
				decoded["threshold"] = json.Number("0")
			},
			field: "threshold",
		},
		{
			name:  "a threshold above the attestor count",
			chain: "ethereum",
			change: func(_ *testing.T, decoded map[string]any) {
				decoded["threshold"] = json.Number("6")
			},
			field: "threshold",
		},
		{
			name:  "a finality depth of zero",
			chain: "ethereum",
			change: func(_ *testing.T, decoded map[string]any) {
				decoded["finality_depth"] = json.Number("0")
			},
			field: "finality_depth",
		},
		{
			name:  "a big-block record on a chain that does not need one",
			chain: "ethereum",
			change: func(_ *testing.T, decoded map[string]any) {
				decoded["big_blocks"] = map[string]any{
					"required":     true,
					"acknowledged": true,
					"requirement":  "none",
				}
			},
			field: "big_blocks",
		},
		{
			name:  "a missing big-block record on the chain that needs one",
			chain: "hyperevm",
			change: func(_ *testing.T, decoded map[string]any) {
				delete(decoded, "big_blocks")
			},
			field: "big_blocks",
		},
		{
			name:  "a big-block requirement declared unnecessary",
			chain: "hyperevm",
			change: func(t *testing.T, decoded map[string]any) {
				section(t, decoded, "big_blocks")["required"] = false
			},
			field: "big_blocks.required",
		},
		{
			name:  "an unwritten big-block requirement",
			chain: "hyperevm",
			change: func(t *testing.T, decoded map[string]any) {
				section(t, decoded, "big_blocks")["requirement"] = "  "
			},
			field: "big_blocks.requirement",
		},
		{
			name:  "a program on an EVM chain",
			chain: "ethereum",
			change: func(_ *testing.T, decoded map[string]any) {
				decoded["solana"] = map[string]any{
					"program_id": SidioraMint,
					"commitment": "finalized",
				}
			},
			field: "solana",
		},
		{
			name:  "a Solana configuration without its program",
			chain: "solana",
			change: func(_ *testing.T, decoded map[string]any) {
				delete(decoded, "solana")
			},
			field: "solana",
		},
		{
			name:  "a commitment that can still be dropped",
			chain: "solana",
			change: func(t *testing.T, decoded map[string]any) {
				section(t, decoded, "solana")["commitment"] = "processed"
			},
			field: "solana.commitment",
		},
		{
			name:  "a malformed program id",
			chain: "solana",
			change: func(t *testing.T, decoded map[string]any) {
				section(t, decoded, "solana")["program_id"] = "not a pubkey"
			},
			field: "solana.program_id",
		},
		{
			name:  "an empty asset list",
			chain: "ethereum",
			change: func(_ *testing.T, decoded map[string]any) {
				decoded["assets"] = []any{}
			},
			field: "assets",
		},
		{
			name:  "a first asset that is not the native coin",
			chain: "ethereum",
			change: func(t *testing.T, decoded map[string]any) {
				first := asset(t, decoded, 0)
				first["address"] = secondEVMAsset()["address"]
				first["asset_id"] = secondEVMAsset()["asset_id"]
			},
			field: "assets[0].address",
		},
		{
			name:  "a native asset carrying another symbol",
			chain: "ethereum",
			change: func(t *testing.T, decoded map[string]any) {
				asset(t, decoded, 0)["symbol"] = "WETH"
			},
			field: "assets[0].symbol",
		},
		{
			name:  "a native asset carrying other decimals",
			chain: "ethereum",
			change: func(t *testing.T, decoded map[string]any) {
				asset(t, decoded, 0)["decimals"] = json.Number("6")
			},
			field: "assets[0].decimals",
		},
		{
			name:  "a per-transaction cap of zero",
			chain: "ethereum",
			change: func(t *testing.T, decoded map[string]any) {
				asset(t, decoded, 0)["per_tx_cap"] = "0"
			},
			field: "assets[0].per_tx_cap",
		},
		{
			name:  "a total cap of zero",
			chain: "ethereum",
			change: func(t *testing.T, decoded map[string]any) {
				asset(t, decoded, 0)["total_cap"] = "0"
			},
			field: "assets[0].total_cap",
		},
		{
			name:  "a per-transaction cap above the total cap",
			chain: "ethereum",
			change: func(t *testing.T, decoded map[string]any) {
				entry := asset(t, decoded, 0)
				entry["per_tx_cap"], entry["total_cap"] = entry["total_cap"], entry["per_tx_cap"]
			},
			field: "assets[0].per_tx_cap",
		},
		{
			name:  "a cap that is not a decimal integer",
			chain: "ethereum",
			change: func(t *testing.T, decoded map[string]any) {
				asset(t, decoded, 0)["per_tx_cap"] = "0x2540be400"
			},
			field: "assets[0].per_tx_cap",
		},
		{
			name:  "the native coin registered twice",
			chain: "ethereum",
			change: func(t *testing.T, decoded map[string]any) {
				second := secondEVMAsset()
				second["address"] = NativeEVMAsset
				second["asset_id"] = NativeEVMAsset
				decoded["assets"] = append(assets(t, decoded), second)
			},
			field: "assets[1].address",
		},
		{
			name:  "an EVM asset id that is not its address",
			chain: "ethereum",
			change: func(t *testing.T, decoded map[string]any) {
				second := secondEVMAsset()
				second["asset_id"] = realOwner
				decoded["assets"] = append(assets(t, decoded), second)
			},
			field: "assets[1].asset_id",
		},
		{
			name:  "a symbol registered twice",
			chain: "ethereum",
			change: func(t *testing.T, decoded map[string]any) {
				second := secondEVMAsset()
				second["symbol"] = asset(t, decoded, 0)["symbol"]
				decoded["assets"] = append(assets(t, decoded), second)
			},
			field: "assets[1].symbol",
		},
		{
			name:  "a first Solana asset that is not the wrapped SOL mint",
			chain: "solana",
			change: func(t *testing.T, decoded map[string]any) {
				list := assets(t, decoded)
				decoded["assets"] = []any{list[1], list[0]}
			},
			field: "assets[0].address",
		},
		{
			name:  "a wrapped SOL asset id that is not the derived handle",
			chain: "solana",
			change: func(t *testing.T, decoded map[string]any) {
				asset(t, decoded, 0)["asset_id"] = realOwner
			},
			field: "assets[0].asset_id",
		},
		{
			name:  "Sidiora's mint carrying its derived handle instead of the fixed id",
			chain: "solana",
			change: func(t *testing.T, decoded map[string]any) {
				handle, err := SolanaHandle(SidioraMint)
				if err != nil {
					t.Fatalf("Sidiora's handle is not derivable: %v", err)
				}
				asset(t, decoded, 1)["asset_id"] = handle
			},
			field: "assets[1].asset_id",
		},
		{
			name:  "Sidiora carrying other decimals",
			chain: "solana",
			change: func(t *testing.T, decoded map[string]any) {
				asset(t, decoded, 1)["decimals"] = json.Number("9")
			},
			field: "assets[1].decimals",
		},
		{
			name:  "another mint carrying the id the chain fixed for Sidiora",
			chain: "solana",
			change: func(t *testing.T, decoded map[string]any) {
				asset(t, decoded, 1)["address"] = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
			},
			field: "assets[1].asset_id",
		},
		{
			name:  "a mint that is not base58",
			chain: "solana",
			change: func(t *testing.T, decoded map[string]any) {
				asset(t, decoded, 1)["address"] = "0OIl" + SidioraMint
			},
			field: "assets[1].address",
		},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			path := copyOf(t, root, test.chain, func(decoded map[string]any) {
				test.change(t, decoded)
			})
			_, err := Load(path)
			if err == nil {
				t.Fatalf("%s is accepted", test.name)
			}
			if !strings.Contains(err.Error(), path) {
				t.Errorf("the refusal does not name the file: %v", err)
			}
			if !strings.Contains(err.Error(), test.field) {
				t.Errorf("the refusal does not name %s: %v", test.field, err)
			}
		})
	}
}

func TestRefusalsOfTheFileItself(t *testing.T) {
	root := repositoryRoot(t)
	t.Run("a chain directory that disagrees with the chain", func(t *testing.T) {
		decoded := document(t, root, "ethereum")
		directory := filepath.Join(t.TempDir(), "base")
		if err := os.MkdirAll(directory, 0o750); err != nil {
			t.Fatalf("the temporary chain directory was not created: %v", err)
		}
		encoded, err := json.MarshalIndent(decoded, "", "  ")
		if err != nil {
			t.Fatalf("the configuration was not encoded: %v", err)
		}
		path := filepath.Join(directory, "config.json")
		if err := os.WriteFile(path, encoded, 0o600); err != nil {
			t.Fatalf("the configuration was not written: %v", err)
		}
		_, err = Load(path)
		if err == nil {
			t.Fatal("a configuration in another chain's directory is accepted")
		}
		if !strings.Contains(err.Error(), path) || !strings.Contains(err.Error(), "chain") {
			t.Errorf("the refusal does not name the file and the field: %v", err)
		}
	})
	t.Run("a file that is not named config.json", func(t *testing.T) {
		decoded := document(t, root, "ethereum")
		encoded, err := json.MarshalIndent(decoded, "", "  ")
		if err != nil {
			t.Fatalf("the configuration was not encoded: %v", err)
		}
		path := filepath.Join(t.TempDir(), "ethereum.json")
		if err := os.WriteFile(path, encoded, 0o600); err != nil {
			t.Fatalf("the configuration was not written: %v", err)
		}
		if _, err := Load(path); err == nil {
			t.Fatal("a configuration outside the chain layout is accepted")
		}
	})
	t.Run("a second JSON document in the file", func(t *testing.T) {
		decoded := document(t, root, "ethereum")
		encoded, err := json.MarshalIndent(decoded, "", "  ")
		if err != nil {
			t.Fatalf("the configuration was not encoded: %v", err)
		}
		directory := filepath.Join(t.TempDir(), "ethereum")
		if err := os.MkdirAll(directory, 0o750); err != nil {
			t.Fatalf("the temporary chain directory was not created: %v", err)
		}
		path := filepath.Join(directory, "config.json")
		if err := os.WriteFile(path, append(encoded, []byte("\n{}\n")...), 0o600); err != nil {
			t.Fatalf("the configuration was not written: %v", err)
		}
		_, err = Load(path)
		if err == nil {
			t.Fatal("a file carrying two JSON documents is accepted")
		}
		if !strings.Contains(err.Error(), "more than one JSON document") {
			t.Errorf("the refusal does not name the cause: %v", err)
		}
	})
	t.Run("a chain that is not a bridge chain", func(t *testing.T) {
		if _, err := Path(repositoryRoot(t), "sepolia"); err == nil {
			t.Error("a chain outside the bridge has a configuration path")
		}
		if _, err := LoadChain(t.TempDir(), "sepolia"); err == nil {
			t.Error("a chain outside the bridge is loadable")
		}
	})
	t.Run("a missing file", func(t *testing.T) {
		if _, err := LoadChain(t.TempDir(), "ethereum"); err == nil {
			t.Error("a missing configuration is accepted")
		}
	})
}

// The handle derivation lives only in bridge/vectors: the configuration reads
// every Solana key through it, so a committed asset id, the pinned vectors and
// the handle the validator derives are one computation, not two that agree.
func TestSolanaHandleIsTheVectorsDerivation(t *testing.T) {
	for _, key := range []vectors.Key32{
		vectors.WrappedSolMint,
		vectors.SidioraMint,
		vectors.VectorProgramID,
		vectors.VectorVaultAuthority,
	} {
		encoded := key.Base58()
		got, err := SolanaHandle(encoded)
		if err != nil {
			t.Fatalf("%s has no handle: %v", encoded, err)
		}
		if want := vectors.Handle(key).Hex(); got != want {
			t.Errorf("the handle of %s is %s here and %s in bridge/vectors", encoded, got, want)
		}
		raw, err := DecodeBase58(encoded)
		if err != nil {
			t.Fatalf("%s does not decode: %v", encoded, err)
		}
		if !bytes.Equal(raw, key[:]) {
			t.Errorf("%s decodes to %x, not %x", encoded, raw, key[:])
		}
	}
	if got, want := strings.ToLower(SidioraAssetID), vectors.SidioraAssetID.Hex(); got != want {
		t.Errorf("Sidiora's asset id is %s here and %s in bridge/vectors", got, want)
	}
	if got, want := SolanaChainID, vectors.SolanaChainID; got != want {
		t.Errorf("Solana's chain id is %d here and %d in bridge/vectors", got, want)
	}
	if got, err := SolanaHandle(vectors.VectorVaultAuthority.Base58()); err != nil || got != vectors.VectorVaultHandle.Hex() {
		t.Errorf("the vault-authority handle is %s (%v), not the pinned %s", got, err, vectors.VectorVaultHandle.Hex())
	}
}
