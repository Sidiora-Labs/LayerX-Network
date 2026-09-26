// Package chainconfig is the schema of the Paxeer X Network bridge chain
// configurations and the loader every bridge tool reads them through.
//
// One schema serves all nine chains: the eight EVM mainnets the PaxeerXVault
// bytecode is deployed to and the Solana chain the custody program is deployed
// to. A configuration carries the chain's identity, the names of the
// environment variables its endpoint and its keys arrive in, the owner, the
// shared attestor set with its threshold, the finality depth and the asset
// list whose first entry is always the chain's native coin. It carries no
// endpoint, no key, no credential and no host: only the names of the
// environment variables those values arrive in.
//
// Reading a configuration happens in two stages, and both of them refuse.
// Load decodes the file with unknown fields forbidden and runs Validate, the
// schema and consistency rules; a committed configuration passes it.
// RequireDeployable is the second stage, the gate a tool that is about to act
// on a chain calls: it refuses the placeholder owner, the placeholder
// attestors and the placeholder program id the committed configurations carry
// by design, and the unacknowledged big-block requirement on hyperevm. There
// is no default owner, no default attestor and no silent substitution: a
// committed configuration is readable by every tool and deployable by none
// until its owner fills in the real values.
package chainconfig

import (
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math/big"
	"os"
	"path/filepath"
	"strings"

	"golang.org/x/crypto/sha3"
)

// Kind is the kind of chain a configuration describes. The kind decides which
// fields the schema requires and how an asset's address and id are read.
type Kind string

const (
	// KindEVM is an EVM mainnet holding custody in the PaxeerXVault contract.
	KindEVM Kind = "evm"
	// KindSolana is the Solana chain holding custody in the bridge program.
	KindSolana Kind = "solana"
)

const (
	// PlaceholderPrefix marks a value the owner has not filled in yet. A
	// placeholder is well formed, so every tool can read the configuration,
	// and RequireDeployable refuses it, so no tool can act on one.
	PlaceholderPrefix = "PLACEHOLDER:"

	// SolanaChainID is Solana's chain id on the Paxeer side: the reserved
	// uint64 91600046870081, the big-endian value of the ASCII bytes SOLANA
	// left-padded to eight bytes (0x0000534f4c414e41). It is far above every
	// EIP-155 chain id in use and cannot collide with one.
	SolanaChainID uint64 = 91600046870081

	// NativeEVMAsset is the asset id of an EVM chain's native coin: address(0),
	// deposited through depositNative and paid out by the native release path.
	NativeEVMAsset = "0x0000000000000000000000000000000000000000"

	// WrappedSolMint is the SPL mint native SOL bridges as, so one code path in
	// the Solana program serves every asset.
	WrappedSolMint = "So11111111111111111111111111111111111111112"

	// SidioraMint is Sidiora's SPL mint. Solana is Sidiora's foreign home.
	SidioraMint = "5w3wVdJaESaJKyLmStM6Hv9UyUkmZ1b9DLQquAqqpump"

	// SidioraAssetID is the 20-byte asset id the chain has already fixed for
	// Sidiora: EnsureSidioraDenom registers the bridged asset (chain id, this
	// address, usid), so the Solana registry records this id for SidioraMint
	// instead of the mint's derived handle and an inbound SID deposit resolves
	// to the denom the bridge module already administers.
	SidioraAssetID = "0x21f7b20a555199fa73A238B1a91FD0f549068fEe"

	// SolanaZeroPubkey is the base58 encoding of the 32 zero bytes, refused
	// wherever a Solana identity is required.
	SolanaZeroPubkey = "11111111111111111111111111111111"

	// MaxAttestors is the largest attestor set the bridge carries: the Solana
	// program's config account holds up to 64 twenty-byte attestor addresses.
	MaxAttestors = 64

	// evmChainsDir and solanaChainsDir are where the configurations live,
	// relative to the repository root.
	evmChainsDir    = "bridge/evm/chains"
	solanaChainsDir = "bridge/solana/chains"

	// configFileName is the file name a chain directory holds.
	configFileName = "config.json"
)

// Chain is one of the nine chains the bridge is deployed to. The registry
// below is the authority a configuration is checked against: a file cannot
// claim a chain id, a native symbol or a native decimal count of its own.
type Chain struct {
	Name           string
	Kind           Kind
	ID             uint64
	NativeSymbol   string
	NativeDecimals uint8
	Dir            string
}

var registry = []Chain{
	{Name: "ethereum", Kind: KindEVM, ID: 1, NativeSymbol: "ETH", NativeDecimals: 18, Dir: evmChainsDir},
	{Name: "base", Kind: KindEVM, ID: 8453, NativeSymbol: "ETH", NativeDecimals: 18, Dir: evmChainsDir},
	{Name: "arbitrum", Kind: KindEVM, ID: 42161, NativeSymbol: "ETH", NativeDecimals: 18, Dir: evmChainsDir},
	{Name: "optimism", Kind: KindEVM, ID: 10, NativeSymbol: "ETH", NativeDecimals: 18, Dir: evmChainsDir},
	{Name: "bnb", Kind: KindEVM, ID: 56, NativeSymbol: "BNB", NativeDecimals: 18, Dir: evmChainsDir},
	{Name: "polygon", Kind: KindEVM, ID: 137, NativeSymbol: "POL", NativeDecimals: 18, Dir: evmChainsDir},
	{Name: "avalanche", Kind: KindEVM, ID: 43114, NativeSymbol: "AVAX", NativeDecimals: 18, Dir: evmChainsDir},
	{Name: "hyperevm", Kind: KindEVM, ID: 999, NativeSymbol: "HYPE", NativeDecimals: 18, Dir: evmChainsDir},
	{Name: "solana", Kind: KindSolana, ID: SolanaChainID, NativeSymbol: "SOL", NativeDecimals: 9, Dir: solanaChainsDir},
}

// BigBlockChain is the one chain whose deployment needs the deploying account
// switched to big blocks first: on Hyperliquid's EVM a contract deployment does
// not fit in a small block.
const BigBlockChain = "hyperevm"

// Chains returns the nine chains of the bridge in deployment order.
func Chains() []Chain {
	out := make([]Chain, len(registry))
	copy(out, registry)
	return out
}

// ChainByName returns the registry entry for a chain name.
func ChainByName(name string) (Chain, bool) {
	for _, chain := range registry {
		if chain.Name == name {
			return chain, true
		}
	}
	return Chain{}, false
}

// Path is where the configuration of a chain lives under a repository root.
func Path(repoRoot, name string) (string, error) {
	chain, ok := ChainByName(name)
	if !ok {
		return "", fmt.Errorf("chainconfig: %q is not a bridge chain", name)
	}
	return filepath.Join(repoRoot, filepath.FromSlash(chain.Dir), chain.Name, configFileName), nil
}

// NativeCoin is the coin a chain pays its own fees in and the coin the default
// pair bridges PAX against.
type NativeCoin struct {
	Symbol   string `json:"symbol"`
	Decimals uint8  `json:"decimals"`
}

// Environment names the environment variables a chain's tools read their
// endpoint and their keys from. The names are committed; the values never are.
type Environment struct {
	// RPCURL names the variable carrying the JSON-RPC endpoint.
	RPCURL string `json:"rpc_url"`
	// DeployKey names the variable carrying the deploy authority: the deployer's
	// key on an EVM chain, the path to the publisher keypair file on Solana.
	DeployKey string `json:"deploy_key"`
	// ExplorerKey names the variable carrying the explorer verification key.
	// EVM chains only: nothing in this feature verifies a Solana program
	// through an explorer key.
	ExplorerKey string `json:"explorer_key,omitempty"`
	// ToolchainBin names the variable carrying the directory holding the pinned
	// solana, solana-keygen and cargo-build-sbf. Solana only.
	ToolchainBin string `json:"toolchain_bin,omitempty"`
}

// BigBlocks records the big-block requirement of a chain whose deployment does
// not fit in a small block, and the owner's acknowledgement that the deploying
// account has been switched. The deploy script refuses to deploy until
// Acknowledged is true.
type BigBlocks struct {
	Required     bool   `json:"required"`
	Acknowledged bool   `json:"acknowledged"`
	Requirement  string `json:"requirement"`
}

// SolanaSection carries what only the Solana chain has: the custody program and
// the commitment its state is read at. The slot finality depth is the chain
// configuration's FinalityDepth, counted in slots on this chain.
type SolanaSection struct {
	ProgramID  string `json:"program_id"`
	Commitment string `json:"commitment"`
}

// Asset is one asset the bridge admits on a chain. Address is the ERC20 address
// on an EVM chain and the SPL mint on Solana; AssetID is the 20 bytes that
// asset enters the attestation digests as.
type Asset struct {
	Symbol   string `json:"symbol"`
	Address  string `json:"address"`
	AssetID  string `json:"asset_id"`
	Decimals uint8  `json:"decimals"`
	PerTxCap string `json:"per_tx_cap"`
	TotalCap string `json:"total_cap"`
}

// PerTx is the per-transaction cap in the asset's base units.
func (a Asset) PerTx() (*big.Int, error) { return parseAmount(a.PerTxCap) }

// Total is the cap on the total amount held in custody, in base units.
func (a Asset) Total() (*big.Int, error) { return parseAmount(a.TotalCap) }

// ChainConfig is one chain's configuration: everything that differs between
// two chains of the bridge, and nothing that differs between two runs.
type ChainConfig struct {
	Chain         string         `json:"chain"`
	Kind          Kind           `json:"kind"`
	ChainID       uint64         `json:"chain_id"`
	Native        NativeCoin     `json:"native"`
	Environment   Environment    `json:"environment"`
	Owner         string         `json:"owner"`
	Attestors     []string       `json:"attestors"`
	Threshold     uint32         `json:"threshold"`
	FinalityDepth uint64         `json:"finality_depth"`
	BigBlocks     *BigBlocks     `json:"big_blocks,omitempty"`
	Solana        *SolanaSection `json:"solana,omitempty"`
	Assets        []Asset        `json:"assets"`

	path string
}

// Source is the file the configuration was loaded from, which every refusal
// names.
func (c *ChainConfig) Source() string { return c.path }

// NativeAsset is the first asset of the configuration, the chain's native coin.
// It is only meaningful for a configuration Validate has accepted.
func (c *ChainConfig) NativeAsset() Asset { return c.Assets[0] }

// Load reads a chain configuration, refusing an unknown field, a second JSON
// document in the file and everything Validate refuses. Every refusal names
// the file and the field.
func Load(path string) (*ChainConfig, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, fmt.Errorf("%s: %w", path, err)
	}
	defer file.Close()

	config := &ChainConfig{path: path}
	decoder := json.NewDecoder(file)
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(config); err != nil {
		return nil, fmt.Errorf("%s: %w", path, err)
	}
	if err := decoder.Decode(new(json.RawMessage)); !errors.Is(err, io.EOF) {
		return nil, fmt.Errorf("%s: the file carries more than one JSON document", path)
	}
	if err := config.Validate(); err != nil {
		return nil, err
	}
	return config, nil
}

// LoadChain reads the configuration of a named chain from the layout under a
// chains root: <root>/<chain>/config.json.
func LoadChain(chainsRoot, name string) (*ChainConfig, error) {
	if _, ok := ChainByName(name); !ok {
		return nil, fmt.Errorf("chainconfig: %q is not a bridge chain", name)
	}
	return Load(filepath.Join(chainsRoot, name, configFileName))
}

// IsPlaceholder reports whether a value is a placeholder the owner has still to
// fill in.
func IsPlaceholder(value string) bool { return strings.HasPrefix(value, PlaceholderPrefix) }

// SolanaHandle derives the 20-byte handle a 32-byte Solana key enters the
// attestation digests as: the last 20 bytes of keccak256 of the key.
func SolanaHandle(key string) (string, error) {
	raw, err := DecodeBase58(key)
	if err != nil {
		return "", err
	}
	if len(raw) != 32 {
		return "", fmt.Errorf("a Solana key is 32 bytes, %q decodes to %d", key, len(raw))
	}
	digest := sha3.NewLegacyKeccak256()
	digest.Write(raw)
	sum := digest.Sum(nil)
	return "0x" + hex.EncodeToString(sum[12:]), nil
}

const base58Alphabet = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"

// DecodeBase58 decodes a base58 Solana key into its bytes, refusing an empty
// string and any character outside the Bitcoin base58 alphabet Solana uses.
func DecodeBase58(value string) ([]byte, error) {
	if value == "" {
		return nil, errors.New("a base58 key is not the empty string")
	}
	number := new(big.Int)
	radix := big.NewInt(58)
	for index, character := range value {
		position := strings.IndexRune(base58Alphabet, character)
		if position < 0 {
			return nil, fmt.Errorf("%q carries %q at byte %d, which is not a base58 character", value, character, index)
		}
		number.Mul(number, radix)
		number.Add(number, big.NewInt(int64(position)))
	}
	leading := len(value) - len(strings.TrimLeft(value, "1"))
	decoded := make([]byte, leading, leading+len(value))
	return append(decoded, number.Bytes()...), nil
}

// parseAmount reads a cap: a decimal integer in the asset's base units, written
// as a JSON string because a uint256 cap does not fit a JSON number.
func parseAmount(value string) (*big.Int, error) {
	if value == "" {
		return nil, errors.New("an amount is a decimal integer, not the empty string")
	}
	for index, character := range value {
		if character < '0' || character > '9' {
			return nil, fmt.Errorf("%q carries %q at byte %d, which is not a decimal digit", value, character, index)
		}
	}
	if len(value) > 1 && value[0] == '0' {
		return nil, fmt.Errorf("%q carries a leading zero", value)
	}
	amount, ok := new(big.Int).SetString(value, 10)
	if !ok {
		return nil, fmt.Errorf("%q is not a decimal integer", value)
	}
	return amount, nil
}
