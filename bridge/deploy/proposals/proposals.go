// Package proposals turns one chain configuration into the governance bodies
// that open that chain on Paxeer X Network: the chain registration, the shared
// attestor set and one cap per asset.
//
// The bodies are the bridge module's own message types, marshalled from
// modules/layerxbridge/types, which this package imports and never modifies,
// so a body cannot drift from what the keeper accepts. Nothing here reaches a
// network: the generator reads committed files and writes JSON.
package proposals

import (
	"bytes"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/cosmos/btcutil/base58"
	"github.com/sidiora-labs/paxeer-network/modules/layerxbridge/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

const (
	// SolanaChainID is the reserved chain id Solana is registered under: the
	// ASCII bytes of SOLANA left-padded to eight bytes and read big-endian.
	SolanaChainID uint64 = 0x0000534f4c414e41

	// WrappedSOLMint is the SPL mint of wrapped SOL and so the address of
	// Solana's native coin in a chain configuration.
	WrappedSOLMint = "So11111111111111111111111111111111111111112"

	// EVMNativeAddress is address(0), the address of the native coin of every
	// EVM chain.
	EVMNativeAddress = "0x0000000000000000000000000000000000000000"

	// DefaultManifestPath is the committed attestor-set manifest, relative to
	// the repository root.
	DefaultManifestPath = "bridge/deploy/attestors.json"

	// CommandName is the name of the generator command.
	CommandName = "paxeer-bridge-proposals"

	registerChainFile = "01-register-chain.json"
	setAttestorsFile  = "02-set-attestors.json"
	setCapFileFormat  = "03-set-cap-%02d-%s.json"

	addressHexLength = 2 + 2*len(types.Address20{})

	// solanaKeyLength is the length of an ed25519 public key, which is what a
	// Solana owner is: a key, not a twenty-byte address.
	solanaKeyLength = 32
)

const (
	fieldName          = "name"
	fieldChainID       = "chain_id"
	fieldChain         = "chain"
	fieldVault         = "vault"
	fieldOwner         = "owner"
	fieldAuthority     = "governance_authority"
	fieldAttestors     = "attestors"
	fieldThreshold     = "threshold"
	fieldFinalityDepth = "finality_depth"
	fieldAssets        = "assets"
	fieldSet           = "set"
	fieldDocument      = "(document)"
)

var sidioraAssetID = mustAddress20(types.SidioraRemoteAddress)

// SidioraAssetID is the asset id the chain fixes for Sidiora: the remote
// address EnsureSidioraDenom registers against the module's usid denom, and so
// the asset id Solana's Sidiora cap carries instead of the mint's derived
// handle.
func SidioraAssetID() types.Address20 { return sidioraAssetID }

// FieldError is a refusal naming the file and the field it was read from.
type FieldError struct {
	File  string
	Field string
	Msg   string
}

func (e *FieldError) Error() string { return fmt.Sprintf("%s: %s: %s", e.File, e.Field, e.Msg) }

func refuse(file, field, format string, args ...any) *FieldError {
	return &FieldError{File: file, Field: field, Msg: fmt.Sprintf(format, args...)}
}

// Asset is one remote asset of one chain: the token's address or SPL mint, the
// twenty bytes the bridge identifies it by, its decimals and its two caps as
// decimal integers in the asset's own base units.
type Asset struct {
	Address  string `json:"address"`
	AssetID  string `json:"asset_id"`
	Decimals uint32 `json:"decimals"`
	MaxPerTx string `json:"max_per_tx"`
	MaxTotal string `json:"max_total"`
}

// ChainConfig is the chain configuration the generator reads: the one file the
// deploy scripts validated and deployed from, so what governance registers and
// what is deployed cannot disagree. It carries no endpoint, key or host - only
// the names of the environment variables that hold them - and it is decoded
// with unknown fields forbidden, so an unrecognised field is refused rather
// than ignored.
type ChainConfig struct {
	Name              string   `json:"name"`
	ChainID           uint64   `json:"chain_id"`
	NativeSymbol      string   `json:"native_symbol"`
	NativeDecimals    uint32   `json:"native_decimals"`
	RPCEndpointEnv    string   `json:"rpc_endpoint_env"`
	ExplorerKeyEnv    string   `json:"explorer_key_env"`
	Authority         string   `json:"governance_authority"`
	Owner             string   `json:"owner"`
	Vault             string   `json:"vault"`
	Attestors         []string `json:"attestors"`
	Threshold         uint32   `json:"threshold"`
	FinalityDepth     uint64   `json:"finality_depth"`
	ProgramID         string   `json:"program_id"`
	Commitment        string   `json:"commitment"`
	BigBlocksRequired bool     `json:"big_blocks_required"`
	Assets            []Asset  `json:"assets"`

	source string
}

// LoadChainConfig reads one chain configuration, refusing an unknown field and
// anything after the document.
func LoadChainConfig(path string) (ChainConfig, error) {
	raw, err := os.ReadFile(path)
	if err != nil {
		return ChainConfig{}, err
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	var cfg ChainConfig
	if err := decoder.Decode(&cfg); err != nil {
		return ChainConfig{}, refuse(path, fieldDocument, "%v", err)
	}
	if decoder.More() {
		return ChainConfig{}, refuse(path, fieldDocument, "more than one JSON document")
	}
	cfg.source = path
	return cfg, nil
}

func (c ChainConfig) file() string {
	if c.source == "" {
		return "chain configuration"
	}
	return c.source
}

// nativeAddress is the address a configuration must list first: address(0) on
// an EVM chain, the wrapped SOL mint on Solana.
func (c ChainConfig) nativeAddress() string {
	if c.ChainID == SolanaChainID {
		return WrappedSOLMint
	}
	return EVMNativeAddress
}

// checkOwner refuses an owner nobody has filled in. An EVM chain's owner is a
// twenty-byte address; Solana's owner is the thirty-two byte key the config PDA
// holds, so it is read as base58.
func (c ChainConfig) checkOwner() error {
	if c.ChainID == SolanaChainID {
		_, err := liveKey(c.file(), fieldOwner, c.Owner)
		return err
	}
	_, err := liveAddress(c.file(), fieldOwner, c.Owner)
	return err
}

func (c ChainConfig) isNative(asset Asset) bool {
	if c.ChainID == SolanaChainID {
		return asset.Address == WrappedSOLMint
	}
	return strings.EqualFold(strings.TrimSpace(asset.Address), EVMNativeAddress)
}

// File is one generated body: the name it is written under and the JSON the
// module's own message type marshalled to.
type File struct {
	Name string
	Body []byte
}

// Bundle is the complete set of bodies for one chain, in the order they are
// submitted: the chain, then the attestor set, then one cap per asset with the
// native coin's cap first.
type Bundle struct {
	Chain     string
	Register  types.MsgRegisterChain
	Attestors types.MsgSetAttestors
	Caps      []types.MsgSetCap
}

// Generate builds every body for one chain against the attestor-set manifest.
// It refuses the whole configuration on the first field it cannot trust, so a
// caller never holds a partial bundle.
func Generate(cfg ChainConfig, manifest AttestorManifest) (Bundle, error) {
	if strings.TrimSpace(cfg.Name) == "" {
		return Bundle{}, refuse(cfg.file(), fieldName, "empty chain name")
	}
	// The owner is the vault's or the program's owner on the remote chain. No
	// governance body carries it, and it is checked here all the same: a
	// configuration whose owner nobody has filled in has not been deployed
	// from, so no proposal may be generated from it.
	if err := cfg.checkOwner(); err != nil {
		return Bundle{}, err
	}
	register, err := RegisterChain(cfg)
	if err != nil {
		return Bundle{}, err
	}
	attestors, err := SetAttestors(cfg, manifest)
	if err != nil {
		return Bundle{}, err
	}
	caps, err := SetCaps(cfg)
	if err != nil {
		return Bundle{}, err
	}
	return Bundle{Chain: cfg.Name, Register: register, Attestors: attestors, Caps: caps}, nil
}

// RegisterChain builds the MsgRegisterChain that opens the chain: its id, the
// address of its vault - on Solana the handle of the vault-authority PDA - and
// the number of confirmations attestors wait for before signing.
func RegisterChain(cfg ChainConfig) (types.MsgRegisterChain, error) {
	authority, err := liveAuthority(cfg.file(), fieldAuthority, cfg.Authority)
	if err != nil {
		return types.MsgRegisterChain{}, err
	}
	if cfg.ChainID == 0 {
		return types.MsgRegisterChain{}, refuse(cfg.file(), fieldChainID, "chain id is zero")
	}
	vault, err := liveAddress(cfg.file(), fieldVault, cfg.Vault)
	if err != nil {
		return types.MsgRegisterChain{}, err
	}
	if cfg.FinalityDepth == 0 {
		return types.MsgRegisterChain{}, refuse(cfg.file(), fieldFinalityDepth, "finality depth is zero")
	}
	msg := types.MsgRegisterChain{
		Authority: authority,
		Chain: types.Chain{
			ChainID:       cfg.ChainID,
			Vault:         vault,
			FinalityDepth: cfg.FinalityDepth,
			Enabled:       true,
		},
	}
	if err := msg.ValidateBasic(); err != nil {
		return types.MsgRegisterChain{}, refuse(cfg.file(), fieldChain, "%v", err)
	}
	return msg, nil
}

// SetAttestors builds the MsgSetAttestors that installs the shared attestor
// set and its threshold. The set is checked against the manifest, so a chain
// cannot be opened with a set the manifest does not record. The manifest
// records addresses and a threshold only, so every attestor is declared with a
// zero bond; a bond is declared on the chain, not in a configuration.
func SetAttestors(cfg ChainConfig, manifest AttestorManifest) (types.MsgSetAttestors, error) {
	authority, err := liveAuthority(cfg.file(), fieldAuthority, cfg.Authority)
	if err != nil {
		return types.MsgSetAttestors{}, err
	}
	signers, err := parseSignerList(cfg.file(), cfg.Attestors)
	if err != nil {
		return types.MsgSetAttestors{}, err
	}
	if err := checkThreshold(cfg.file(), cfg.Threshold, len(signers)); err != nil {
		return types.MsgSetAttestors{}, err
	}
	if err := manifest.Check(cfg); err != nil {
		return types.MsgSetAttestors{}, err
	}
	set := types.AttestorSet{Attestors: make([]types.Attestor, 0, len(signers)), Threshold: cfg.Threshold}
	for _, signer := range signers {
		set.Attestors = append(set.Attestors, types.Attestor{Signer: signer, Bond: sdk.NewInt(0)})
	}
	msg := types.MsgSetAttestors{Authority: authority, Set: set}
	if err := msg.ValidateBasic(); err != nil {
		return types.MsgSetAttestors{}, refuse(cfg.file(), fieldSet, "%v", err)
	}
	return msg, nil
}

// SetCaps builds one MsgSetCap per asset, in configuration order. The set
// always begins with the chain's native coin, so the default pair - PAX
// against that coin - works the moment the chain is opened, and the Solana set
// always carries Sidiora's cap for the asset id the chain fixes.
func SetCaps(cfg ChainConfig) ([]types.MsgSetCap, error) {
	authority, err := liveAuthority(cfg.file(), fieldAuthority, cfg.Authority)
	if err != nil {
		return nil, err
	}
	if cfg.ChainID == 0 {
		return nil, refuse(cfg.file(), fieldChainID, "chain id is zero")
	}
	if len(cfg.Assets) == 0 {
		return nil, refuse(cfg.file(), fieldAssets, "no assets")
	}
	if !cfg.isNative(cfg.Assets[0]) {
		return nil, refuse(cfg.file(), fieldAssets+"[0].address",
			"first asset is %q, not the chain's native coin %q", cfg.Assets[0].Address, cfg.nativeAddress())
	}
	caps := make([]types.MsgSetCap, 0, len(cfg.Assets))
	seen := make(map[types.Address20]int, len(cfg.Assets))
	carriesSidiora := false
	for i, asset := range cfg.Assets {
		prefix := fmt.Sprintf("%s[%d].", fieldAssets, i)
		if i > 0 && cfg.isNative(asset) {
			return nil, refuse(cfg.file(), prefix+"address", "the native coin is listed twice")
		}
		id, err := decodeField(cfg.file(), prefix+"asset_id", asset.AssetID)
		if err != nil {
			return nil, err
		}
		if first, duplicate := seen[id]; duplicate {
			return nil, refuse(cfg.file(), prefix+"asset_id",
				"asset id %s is already the asset id of %s[%d]", id.Hex(), fieldAssets, first)
		}
		seen[id] = i
		if id == sidioraAssetID {
			carriesSidiora = true
		}
		perTx, err := liveCap(cfg.file(), prefix+"max_per_tx", asset.MaxPerTx)
		if err != nil {
			return nil, err
		}
		total, err := liveCap(cfg.file(), prefix+"max_total", asset.MaxTotal)
		if err != nil {
			return nil, err
		}
		if perTx.GT(total) {
			return nil, refuse(cfg.file(), prefix+"max_per_tx",
				"per-transaction cap %s is above the total cap %s", perTx.String(), total.String())
		}
		msg := types.MsgSetCap{
			Authority:   authority,
			ChainID:     cfg.ChainID,
			Asset:       id,
			MaxInFlight: total,
			MaxPerTx:    perTx,
		}
		if err := msg.ValidateBasic(); err != nil {
			return nil, refuse(cfg.file(), prefix+"asset_id", "%v", err)
		}
		caps = append(caps, msg)
	}
	if cfg.ChainID == SolanaChainID && !carriesSidiora {
		return nil, refuse(cfg.file(), fieldAssets,
			"the Solana configuration carries no asset with Sidiora's asset id %s", sidioraAssetID.Hex())
	}
	return caps, nil
}

// Files marshals every body of the bundle. Marshalling happens before any file
// is created, so a bundle that cannot be marshalled leaves nothing behind.
func (b Bundle) Files() ([]File, error) {
	files := make([]File, 0, 2+len(b.Caps))
	register, err := marshalBody(b.Register)
	if err != nil {
		return nil, err
	}
	files = append(files, File{Name: registerChainFile, Body: register})
	attestors, err := marshalBody(b.Attestors)
	if err != nil {
		return nil, err
	}
	files = append(files, File{Name: setAttestorsFile, Body: attestors})
	for i, msg := range b.Caps {
		body, err := marshalBody(msg)
		if err != nil {
			return nil, err
		}
		name := fmt.Sprintf(setCapFileFormat, i+1, hex.EncodeToString(msg.Asset[:]))
		files = append(files, File{Name: name, Body: body})
	}
	return files, nil
}

// Write creates dir and writes every body into it, returning the paths it
// wrote in submission order.
func (b Bundle) Write(dir string) ([]string, error) {
	files, err := b.Files()
	if err != nil {
		return nil, err
	}
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return nil, err
	}
	written := make([]string, 0, len(files))
	for _, file := range files {
		path := filepath.Join(dir, file.Name)
		if err := os.WriteFile(path, file.Body, 0o644); err != nil {
			return written, err
		}
		written = append(written, path)
	}
	return written, nil
}

// Run is the paxeer-bridge-proposals command: it reads one chain
// configuration and the committed attestor-set manifest, refuses a
// placeholder or zero authority, owner or attestor, a zero threshold or one
// above the attestor count and a zero cap, and writes the bodies into the
// output directory. Every refusal happens before the output directory is
// touched, so a refused run writes nothing at all.
func Run(args []string, report io.Writer) error {
	flags := flag.NewFlagSet(CommandName, flag.ContinueOnError)
	flags.SetOutput(report)
	manifestPath := flags.String("manifest", DefaultManifestPath,
		"path of the committed attestor-set manifest")
	flags.Usage = func() {
		fmt.Fprintf(report, "usage: %s [-manifest <path>] <chain-configuration.json> <output-directory>\n", CommandName)
		flags.PrintDefaults()
	}
	if err := flags.Parse(args); err != nil {
		return err
	}
	if flags.NArg() != 2 {
		flags.Usage()
		return fmt.Errorf("%s: expected a chain configuration path and an output directory, got %d arguments",
			CommandName, flags.NArg())
	}
	manifest, err := LoadAttestorManifest(*manifestPath)
	if err != nil {
		return err
	}
	cfg, err := LoadChainConfig(flags.Arg(0))
	if err != nil {
		return err
	}
	bundle, err := Generate(cfg, manifest)
	if err != nil {
		return err
	}
	written, err := bundle.Write(flags.Arg(1))
	if err != nil {
		return err
	}
	for _, path := range written {
		fmt.Fprintln(report, path)
	}
	return nil
}

func marshalBody(body any) ([]byte, error) {
	raw, err := json.MarshalIndent(body, "", "  ")
	if err != nil {
		return nil, err
	}
	return append(raw, '\n'), nil
}

// isPlaceholder reports whether every byte is the same non-zero byte. The
// committed manifest and the committed chain configurations use such values
// where an address is not known yet, so every tool refuses a file nobody has
// filled in and no tool can mistake one for an address in use.
func isPlaceholder(raw []byte) bool {
	if len(raw) == 0 || raw[0] == 0 {
		return false
	}
	for _, b := range raw {
		if b != raw[0] {
			return false
		}
	}
	return true
}

func isZero(raw []byte) bool {
	for _, b := range raw {
		if b != 0 {
			return false
		}
	}
	return true
}

func decodeAddress20(text string) (types.Address20, error) {
	trimmed := strings.TrimSpace(text)
	if !strings.HasPrefix(trimmed, "0x") {
		return types.Address20{}, fmt.Errorf("%q is not a 0x-prefixed address", trimmed)
	}
	if len(trimmed) != addressHexLength {
		return types.Address20{}, fmt.Errorf("%q is %d characters, not %d", trimmed, len(trimmed), addressHexLength)
	}
	raw, err := hex.DecodeString(trimmed[2:])
	if err != nil {
		return types.Address20{}, fmt.Errorf("%q is not hexadecimal: %w", trimmed, err)
	}
	return types.Address20(raw), nil
}

func mustAddress20(text string) types.Address20 {
	address, err := decodeAddress20(text)
	if err != nil {
		panic(err)
	}
	return address
}

// decodeField parses a twenty-byte value and refuses anything but the exact
// shape. The zero value is accepted: address(0) is the asset id of the native
// coin of every EVM chain.
func decodeField(file, field, text string) (types.Address20, error) {
	address, err := decodeAddress20(text)
	if err != nil {
		return types.Address20{}, refuse(file, field, "%v", err)
	}
	return address, nil
}

// liveAddress parses a twenty-byte address that must name something real: a
// zero or placeholder value is refused.
func liveAddress(file, field, text string) (types.Address20, error) {
	address, err := decodeField(file, field, text)
	if err != nil {
		return types.Address20{}, err
	}
	if isZero(address[:]) {
		return types.Address20{}, refuse(file, field, "zero address")
	}
	if isPlaceholder(address[:]) {
		return types.Address20{}, refuse(file, field, "placeholder address %s", address.Hex())
	}
	return address, nil
}

// liveKey parses a base58 ed25519 public key that must name something real: a
// zero or placeholder key is refused.
func liveKey(file, field, text string) ([]byte, error) {
	trimmed := strings.TrimSpace(text)
	if trimmed == "" {
		return nil, refuse(file, field, "empty key")
	}
	raw := base58.Decode(trimmed)
	if len(raw) != solanaKeyLength {
		return nil, refuse(file, field, "%q is not a base58 %d-byte key", trimmed, solanaKeyLength)
	}
	if isZero(raw) {
		return nil, refuse(file, field, "zero key")
	}
	if isPlaceholder(raw) {
		return nil, refuse(file, field, "placeholder key %q", trimmed)
	}
	return raw, nil
}

// liveAuthority parses the governance authority, the only account the keeper
// executes these messages for.
func liveAuthority(file, field, text string) (string, error) {
	trimmed := strings.TrimSpace(text)
	if trimmed == "" {
		return "", refuse(file, field, "empty governance authority")
	}
	account, err := sdk.AccAddressFromBech32(trimmed)
	if err != nil {
		return "", refuse(file, field, "%q is not a bech32 account address: %v", trimmed, err)
	}
	if isZero(account) {
		return "", refuse(file, field, "zero governance authority")
	}
	if isPlaceholder(account) {
		return "", refuse(file, field, "placeholder governance authority %q", trimmed)
	}
	return trimmed, nil
}

// liveCap parses a cap: a decimal integer in the asset's base units that is
// neither negative nor zero, because a zero cap refuses every bridgeIn.
func liveCap(file, field, text string) (sdk.Int, error) {
	trimmed := strings.TrimSpace(text)
	if trimmed == "" {
		return sdk.Int{}, refuse(file, field, "empty cap")
	}
	value, ok := sdk.NewIntFromString(trimmed)
	if !ok {
		return sdk.Int{}, refuse(file, field, "%q is not a decimal integer", trimmed)
	}
	if value.IsNegative() {
		return sdk.Int{}, refuse(file, field, "negative cap %s", value.String())
	}
	if value.IsZero() {
		return sdk.Int{}, refuse(file, field, "zero cap")
	}
	return value, nil
}
