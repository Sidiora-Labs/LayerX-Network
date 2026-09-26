// Package proposals turns one chain configuration into the governance bodies
// that open that chain on Paxeer X Network: the chain registration, the shared
// attestor set and one cap per asset.
//
// The bodies are the bridge module's own generated message types from
// modules/layerxbridge/types, which this package imports and never modifies,
// each written as the protobuf JSON of a message packed under its type URL -
// the form a governance proposal or a transaction carries its messages in - so
// a body cannot drift from what the keeper accepts. Given -proposals it also
// writes the module's governance proposal content, BridgeProposal, carrying
// the same messages in submission order, which the chain's submit-proposal
// transaction carries as its content as it stands. Nothing here reaches a
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
	"github.com/sidiora-labs/paxeer-network/bridge/deploy/chainconfig"
	"github.com/sidiora-labs/paxeer-network/bridge/vectors"
	"github.com/sidiora-labs/paxeer-network/modules/layerxbridge/types"
	"github.com/sidiora-labs/paxeer-network/sdk/codec"
	cdctypes "github.com/sidiora-labs/paxeer-network/sdk/codec/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	govtypes "github.com/sidiora-labs/paxeer-network/sdk/x/gov/types"
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

	// OpenChainProposalFile is the proposal that opens the chain: its
	// registration, the attestor set and every cap but Sidiora's.
	OpenChainProposalFile = "04-proposal-open-chain.json"

	// SidioraCapProposalFile is Solana's second proposal: it registers the
	// Sidiora pair against the usid denom and then sets Sidiora's cap, in that
	// order, once the opening proposal has registered the chain.
	SidioraCapProposalFile = "05-proposal-sidiora-cap.json"

	addressHexLength = 2 + 2*len(types.Address20{})

	// solanaKeyLength is the length of an ed25519 public key, which is what a
	// Solana owner is: a key, not a twenty-byte address.
	solanaKeyLength = 32

	// solanaConfigSeed and solanaAssetSeed are the seeds the custody program
	// derives its config account and each asset account from, as
	// bridge/solana/src/state.rs declares them.
	solanaConfigSeed = "config"
	solanaAssetSeed  = "asset"
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

// messageCodec packs and unpacks the module's messages under their type URLs,
// resolved against the module's own interface registration.
var messageCodec = newMessageCodec()

func newMessageCodec() *codec.ProtoCodec {
	registry := cdctypes.NewInterfaceRegistry()
	govtypes.RegisterInterfaces(registry)
	types.RegisterInterfaces(registry)
	return codec.NewProtoCodec(registry)
}

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

// File is one generated body: the name it is written under and the JSON of
// the module's own message packed under its type URL.
type File struct {
	Name string
	Body []byte
}

// Bundle is the complete set of bodies for one chain, in the order they are
// submitted: the chain, then the attestor set, then one cap per asset with the
// native coin's cap first. On Solana, Sidiora's foreign home, it also carries
// the registration of the Sidiora pair, which the Sidiora proposal executes
// ahead of Sidiora's cap.
type Bundle struct {
	Chain       string
	Register    types.MsgRegisterChain
	Attestors   types.MsgSetAttestors
	Caps        []types.MsgSetCap
	SidioraPair *types.MsgRegisterSidioraPair
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
	bundle := Bundle{Chain: cfg.Name, Register: register, Attestors: attestors, Caps: caps}
	if cfg.ChainID == SolanaChainID {
		pair, err := RegisterSidioraPair(cfg)
		if err != nil {
			return Bundle{}, err
		}
		bundle.SidioraPair = &pair
	}
	return bundle, nil
}

// RegisterSidioraPair builds the MsgRegisterSidioraPair that records Sidiora's
// asset id on Solana against the module's usid denom. It refuses every chain
// but Solana, Sidiora's foreign home.
func RegisterSidioraPair(cfg ChainConfig) (types.MsgRegisterSidioraPair, error) {
	authority, err := liveAuthority(cfg.file(), fieldAuthority, cfg.Authority)
	if err != nil {
		return types.MsgRegisterSidioraPair{}, err
	}
	if cfg.ChainID != SolanaChainID {
		return types.MsgRegisterSidioraPair{}, refuse(cfg.file(), fieldChainID,
			"chain %d is not Solana, chain %d, Sidiora's foreign home", cfg.ChainID, SolanaChainID)
	}
	msg := types.MsgRegisterSidioraPair{Authority: authority, ChainID: cfg.ChainID}
	if err := msg.ValidateBasic(); err != nil {
		return types.MsgRegisterSidioraPair{}, refuse(cfg.file(), fieldChainID, "%v", err)
	}
	return msg, nil
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
			if cfg.ChainID != SolanaChainID {
				return nil, refuse(cfg.file(), prefix+"asset_id",
					"Sidiora's asset id %s is bridged only from Solana, Sidiora's foreign home", id.Hex())
			}
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
	register, err := marshalBody(&b.Register)
	if err != nil {
		return nil, err
	}
	files = append(files, File{Name: registerChainFile, Body: register})
	attestors, err := marshalBody(&b.Attestors)
	if err != nil {
		return nil, err
	}
	files = append(files, File{Name: setAttestorsFile, Body: attestors})
	for i := range b.Caps {
		msg := &b.Caps[i]
		body, err := marshalBody(msg)
		if err != nil {
			return nil, err
		}
		name := fmt.Sprintf(setCapFileFormat, i+1, hex.EncodeToString(msg.Asset[:]))
		files = append(files, File{Name: name, Body: body})
	}
	return files, nil
}

// ProposalFiles marshals every proposal of the bundle, in submission order.
func (b Bundle) ProposalFiles() ([]File, error) {
	proposals, err := b.Proposals()
	if err != nil {
		return nil, err
	}
	files := make([]File, 0, len(proposals))
	for _, proposal := range proposals {
		body, err := marshalProposal(proposal.Content)
		if err != nil {
			return nil, err
		}
		files = append(files, File{Name: proposal.Name, Body: body})
	}
	return files, nil
}

// Proposal is one governance proposal of a bundle: the name it is written
// under and the module's proposal content.
type Proposal struct {
	Name    string
	Content *types.BridgeProposal
}

// Proposals builds the bundle's governance proposals in submission order. The
// first opens the chain: its registration, the attestor set and every cap in
// configuration order, the native coin's first. Sidiora's cap is never part of
// it. A cap for a pair the Paxeer side has not recorded registers the pair
// under the bridge's generic denom, so Sidiora's cap travels in a second
// proposal that first registers the pair against usid and then caps it; it
// needs the chain the first proposal registers, and it is built only for
// Solana, Sidiora's foreign home.
func (b Bundle) Proposals() ([]Proposal, error) {
	open := []sdk.Msg{&b.Register, &b.Attestors}
	var sidiora *types.MsgSetCap
	for i := range b.Caps {
		capMsg := &b.Caps[i]
		if capMsg.Asset == sidioraAssetID {
			sidiora = capMsg
			continue
		}
		open = append(open, capMsg)
	}
	set := b.Attestors.Set
	chain := b.Register.Chain
	content, err := types.NewBridgeProposal(
		fmt.Sprintf("Open %s on the Paxeer X Network bridge", b.Chain),
		fmt.Sprintf("Registers %s as chain %d with the vault %s and a finality depth of %d, installs the shared "+
			"attestor set of %d attestors with a threshold of %d, and sets the caps of %d assets, the native coin first.",
			b.Chain, chain.ChainID, chain.Vault.Hex(), chain.FinalityDepth, len(set.Attestors), set.Threshold, len(open)-2),
		open...)
	if err != nil {
		return nil, err
	}
	proposals := []Proposal{{Name: OpenChainProposalFile, Content: content}}
	if sidiora == nil {
		return proposals, nil
	}
	pair := b.SidioraPair
	if pair == nil || pair.ChainID != SolanaChainID || sidiora.ChainID != pair.ChainID {
		return nil, fmt.Errorf("%s: Sidiora's cap on chain %d travels only with the registration of its pair on Solana, chain %d, Sidiora's foreign home",
			b.Chain, sidiora.ChainID, SolanaChainID)
	}
	content, err = types.NewBridgeProposal(
		fmt.Sprintf("Register and cap Sidiora on the %s bridge", b.Chain),
		fmt.Sprintf("Registers the Sidiora pair, asset id %s on chain %d, against the usid denom %s, and then sets its cap "+
			"to %s per transaction and %s in flight. Submitted once the proposal that opens chain %d has passed.",
			sidiora.Asset.Hex(), sidiora.ChainID, types.SidioraDenom(), sidiora.MaxPerTx.String(), sidiora.MaxInFlight.String(),
			sidiora.ChainID),
		pair, sidiora)
	if err != nil {
		return nil, err
	}
	return append(proposals, Proposal{Name: SidioraCapProposalFile, Content: content}), nil
}

// Write writes every body into dir, returning the paths it wrote in
// submission order. It refuses a dir that already holds anything, so no body
// of an earlier bundle can sit beside this one, and it writes the bodies into
// a temporary sibling directory that takes dir's place only once every body is
// on disk, so a failed write leaves no partial bundle at dir.
func (b Bundle) Write(dir string) ([]string, error) {
	files, err := b.Files()
	if err != nil {
		return nil, err
	}
	return writeFiles(dir, files)
}

// WriteProposals writes every proposal into dir the way Write writes the
// bodies: into an empty or absent directory, all at once or not at all.
func (b Bundle) WriteProposals(dir string) ([]string, error) {
	files, err := b.ProposalFiles()
	if err != nil {
		return nil, err
	}
	return writeFiles(dir, files)
}

// requireEmpty refuses a directory that already holds anything.
func requireEmpty(dir string) error {
	entries, err := os.ReadDir(dir)
	switch {
	case err == nil && len(entries) > 0:
		return fmt.Errorf("%s: the output directory is not empty", dir)
	case err != nil && !os.IsNotExist(err):
		return err
	}
	return nil
}

func writeFiles(dir string, files []File) ([]string, error) {
	dir = filepath.Clean(dir)
	if err := requireEmpty(dir); err != nil {
		return nil, err
	}
	parent := filepath.Dir(dir)
	if err := os.MkdirAll(parent, 0o755); err != nil {
		return nil, err
	}
	staging, err := os.MkdirTemp(parent, ".proposals-*")
	if err != nil {
		return nil, err
	}
	defer os.RemoveAll(staging)
	if err := os.Chmod(staging, 0o755); err != nil {
		return nil, err
	}
	for _, file := range files {
		if err := os.WriteFile(filepath.Join(staging, file.Name), file.Body, 0o644); err != nil {
			return nil, err
		}
	}
	if err := os.Remove(dir); err != nil && !os.IsNotExist(err) {
		return nil, err
	}
	if err := os.Rename(staging, dir); err != nil {
		return nil, err
	}
	written := make([]string, 0, len(files))
	for _, file := range files {
		written = append(written, filepath.Join(dir, file.Name))
	}
	return written, nil
}

// Run is the paxeer-bridge-proposals command: it reads one chain
// configuration and the committed attestor-set manifest, refuses a
// placeholder or zero authority, owner or attestor, a zero threshold or one
// above the attestor count and a zero cap, refuses an output directory that
// already holds anything, and writes the bodies into the output directory.
// Every refusal happens before the output directory is touched, so a refused
// run writes nothing at all.
//
// Given -authority and -vault, the configuration is read through
// bridge/deploy/chainconfig, the bridge's one configuration schema, and the two
// values no configuration carries - the governance authority and the deployed
// vault - arrive as arguments. -readback then also writes the values the
// deployed chain and the Paxeer side must read back once the bodies execute.
//
// Given -proposals, it also writes the bundle's governance proposals into that
// directory: the proposal that opens the chain and, on Solana, the proposal
// that registers the Sidiora pair against usid and then sets Sidiora's cap. Each is the content a
// submit-proposal transaction carries as it stands.
func Run(args []string, report io.Writer) error {
	flags := flag.NewFlagSet(CommandName, flag.ContinueOnError)
	flags.SetOutput(report)
	manifestPath := flags.String("manifest", DefaultManifestPath,
		"path of the committed attestor-set manifest")
	authority := flags.String("authority", "",
		"bech32 governance authority the bodies are executed for; reads the configuration through bridge/deploy/chainconfig")
	vault := flags.String("vault", "",
		"deployed vault address, on Solana the vault-authority handle; reads the configuration through bridge/deploy/chainconfig")
	readbackPath := flags.String("readback", "",
		"path the read-back expectation is written to; needs -authority and -vault")
	proposalsPath := flags.String("proposals", "",
		"directory the governance proposals are written to, apart from the bodies")
	flags.Usage = func() {
		fmt.Fprintf(report, "usage: %s [-manifest <path>] [-authority <bech32> -vault <address> [-readback <path>]] [-proposals <directory>] <chain-configuration.json> <output-directory>\n", CommandName)
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
	canonical := *authority != "" || *vault != "" || *readbackPath != ""
	if canonical && (*authority == "" || *vault == "") {
		return fmt.Errorf("%s: -authority and -vault are both required to read a chain configuration through chainconfig", CommandName)
	}
	manifest, err := LoadAttestorManifest(*manifestPath)
	if err != nil {
		return err
	}
	var (
		cfg       ChainConfig
		canonCfg  *chainconfig.ChainConfig
		expected  []byte
		readbackF string
	)
	if canonical {
		canonCfg, err = chainconfig.Load(flags.Arg(0))
		if err != nil {
			return err
		}
		cfg, err = FromChainConfig(canonCfg, *authority, *vault)
	} else {
		cfg, err = LoadChainConfig(flags.Arg(0))
	}
	if err != nil {
		return err
	}
	bundle, err := Generate(cfg, manifest)
	if err != nil {
		return err
	}
	var proposalsDir string
	if *proposalsPath != "" {
		proposalsDir = filepath.Clean(*proposalsPath)
		if proposalsDir == filepath.Clean(flags.Arg(1)) {
			return fmt.Errorf("%s: the proposals are written apart from the bodies, not into %s", CommandName, proposalsDir)
		}
		if _, err := bundle.ProposalFiles(); err != nil {
			return err
		}
		if err := requireEmpty(proposalsDir); err != nil {
			return err
		}
	}
	if *readbackPath != "" {
		readback, err := ReadbackOf(canonCfg, bundle)
		if err != nil {
			return err
		}
		if expected, err = marshalJSON(readback); err != nil {
			return err
		}
		readbackF = filepath.Clean(*readbackPath)
		if _, err := os.Lstat(readbackF); err == nil {
			return fmt.Errorf("%s: the read-back expectation already exists", readbackF)
		} else if !os.IsNotExist(err) {
			return err
		}
	}
	written, err := bundle.Write(flags.Arg(1))
	if err != nil {
		return err
	}
	if proposalsDir != "" {
		proposalPaths, err := bundle.WriteProposals(proposalsDir)
		if err != nil {
			return err
		}
		written = append(written, proposalPaths...)
	}
	if readbackF != "" {
		if err := writeAtomically(readbackF, expected); err != nil {
			return err
		}
		written = append(written, readbackF)
	}
	for _, path := range written {
		fmt.Fprintln(report, path)
	}
	return nil
}

// writeAtomically writes body to a temporary sibling of path and renames it
// into place, so a failed write leaves nothing at path.
func writeAtomically(path string, body []byte) error {
	file, err := os.CreateTemp(filepath.Dir(path), ".readback-*")
	if err != nil {
		return err
	}
	staged := file.Name()
	defer os.Remove(staged)
	if _, err := file.Write(body); err != nil {
		file.Close()
		return err
	}
	if err := file.Close(); err != nil {
		return err
	}
	if err := os.Chmod(staged, 0o644); err != nil {
		return err
	}
	return os.Rename(staged, path)
}

// FromChainConfig is the generator's reading of a configuration in the
// bridge's one schema, bridge/deploy/chainconfig. The configuration must be
// deployable - no placeholder owner, attestor or program id - and the two
// values a configuration does not carry arrive beside it: the bech32 governance
// authority the keeper executes the bodies for, and the deployed vault. On
// Solana the vault is the handle of the program's vault-authority PDA, and a
// vault that is not that handle is refused, because governance would register
// an address no release can come from.
func FromChainConfig(cfg *chainconfig.ChainConfig, authority, vault string) (ChainConfig, error) {
	if err := cfg.RequireDeployable(); err != nil {
		return ChainConfig{}, err
	}
	out := ChainConfig{
		Name:           cfg.Chain,
		ChainID:        cfg.ChainID,
		NativeSymbol:   cfg.Native.Symbol,
		NativeDecimals: uint32(cfg.Native.Decimals),
		RPCEndpointEnv: cfg.Environment.RPCURL,
		ExplorerKeyEnv: cfg.Environment.ExplorerKey,
		Authority:      authority,
		Owner:          cfg.Owner,
		Vault:          vault,
		Attestors:      append([]string(nil), cfg.Attestors...),
		Threshold:      cfg.Threshold,
		FinalityDepth:  cfg.FinalityDepth,
		Assets:         make([]Asset, 0, len(cfg.Assets)),
		source:         cfg.Source(),
	}
	if cfg.BigBlocks != nil {
		out.BigBlocksRequired = cfg.BigBlocks.Required
	}
	for _, asset := range cfg.Assets {
		out.Assets = append(out.Assets, Asset{
			Address:  asset.Address,
			AssetID:  asset.AssetID,
			Decimals: uint32(asset.Decimals),
			MaxPerTx: asset.PerTxCap,
			MaxTotal: asset.TotalCap,
		})
	}
	if cfg.Solana == nil {
		return out, nil
	}
	out.ProgramID = cfg.Solana.ProgramID
	out.Commitment = cfg.Solana.Commitment
	program, err := vectors.Key(cfg.Solana.ProgramID)
	if err != nil {
		return ChainConfig{}, refuse(out.file(), "solana.program_id", "%v", err)
	}
	handle, err := vectors.VaultHandle(program)
	if err != nil {
		return ChainConfig{}, refuse(out.file(), "solana.program_id", "%v", err)
	}
	given, err := liveAddress(out.file(), fieldVault, vault)
	if err != nil {
		return ChainConfig{}, err
	}
	if given != types.Address20(handle) {
		return ChainConfig{}, refuse(out.file(), fieldVault,
			"%s is not %s, the handle of the vault-authority PDA of program %s", given.Hex(), handle.Hex(), cfg.Solana.ProgramID)
	}
	return out, nil
}

// Readback is what a deployed chain and the Paxeer side must read back once a
// chain is deployed from its configuration and its bundle has executed: the
// owner, the attestor set and threshold, the vault registered on Paxeer with
// its finality depth, and per asset, native coin first, the caps and the denom
// the Paxeer side mints it as. On Solana it also names the accounts the
// program keeps that state in, derived the way the program derives them.
type Readback struct {
	Chain         string          `json:"chain"`
	Kind          string          `json:"kind"`
	ChainID       uint64          `json:"chain_id"`
	Owner         string          `json:"owner"`
	Vault         string          `json:"vault"`
	FinalityDepth uint64          `json:"finality_depth"`
	Attestors     []string        `json:"attestors"`
	Threshold     uint32          `json:"threshold"`
	Solana        *SolanaReadback `json:"solana,omitempty"`
	Assets        []AssetReadback `json:"assets"`
}

// SolanaReadback names the custody program and the accounts it keeps its
// configuration and its custody authority in.
type SolanaReadback struct {
	ProgramID      string `json:"program_id"`
	Commitment     string `json:"commitment"`
	ConfigAccount  string `json:"config_account"`
	VaultAuthority string `json:"vault_authority"`
}

// AssetReadback is one asset as it must read back: its caps as the bodies set
// them and the denom the Paxeer side mints it as. Mint and Account are the
// Solana mint's bytes in hex and the program's asset account of that mint.
type AssetReadback struct {
	Symbol   string `json:"symbol"`
	Address  string `json:"address"`
	AssetID  string `json:"asset_id"`
	Decimals uint8  `json:"decimals"`
	PerTxCap string `json:"per_tx_cap"`
	TotalCap string `json:"total_cap"`
	Denom    string `json:"denom"`
	Mint     string `json:"mint,omitempty"`
	Account  string `json:"account,omitempty"`
}

// ReadbackOf derives the read-back expectation of a chain from its
// configuration and the bundle generated from it. Every cap comes from the
// bundle's own bodies, in their order, so what the checklist compares against
// is what governance submits. Sidiora on Solana reads back as the module's
// usid denom, the pair the Sidiora proposal registers ahead of its cap; every other
// asset reads back as the tokenfactory denom of its (chain, asset) pair.
func ReadbackOf(cfg *chainconfig.ChainConfig, bundle Bundle) (Readback, error) {
	if cfg == nil {
		return Readback{}, fmt.Errorf("%s: a read-back expectation is derived from a chainconfig configuration", CommandName)
	}
	if bundle.Chain != cfg.Chain || bundle.Register.Chain.ChainID != cfg.ChainID {
		return Readback{}, refuse(cfg.Source(), fieldChain, "the bundle is for %s (%d), not %s (%d)",
			bundle.Chain, bundle.Register.Chain.ChainID, cfg.Chain, cfg.ChainID)
	}
	if len(bundle.Caps) != len(cfg.Assets) {
		return Readback{}, refuse(cfg.Source(), fieldAssets, "the bundle carries %d caps for %d assets",
			len(bundle.Caps), len(cfg.Assets))
	}
	out := Readback{
		Chain:         cfg.Chain,
		Kind:          string(cfg.Kind),
		ChainID:       cfg.ChainID,
		Owner:         strings.ToLower(cfg.Owner),
		Vault:         bundle.Register.Chain.Vault.Hex(),
		FinalityDepth: bundle.Register.Chain.FinalityDepth,
		Attestors:     make([]string, 0, len(bundle.Attestors.Set.Attestors)),
		Threshold:     bundle.Attestors.Set.Threshold,
		Assets:        make([]AssetReadback, 0, len(cfg.Assets)),
	}
	for _, attestor := range bundle.Attestors.Set.Attestors {
		out.Attestors = append(out.Attestors, attestor.Signer.Hex())
	}
	var program vectors.Key32
	if cfg.Solana != nil {
		var err error
		if program, err = vectors.Key(cfg.Solana.ProgramID); err != nil {
			return Readback{}, refuse(cfg.Source(), "solana.program_id", "%v", err)
		}
		owner, err := vectors.Key(cfg.Owner)
		if err != nil {
			return Readback{}, refuse(cfg.Source(), fieldOwner, "%v", err)
		}
		out.Owner = "0x" + hex.EncodeToString(owner[:])
		configAccount, _, err := vectors.FindProgramAddress(program, [][]byte{[]byte(solanaConfigSeed)})
		if err != nil {
			return Readback{}, refuse(cfg.Source(), "solana.program_id", "%v", err)
		}
		authority, _, err := vectors.VaultAuthority(program)
		if err != nil {
			return Readback{}, refuse(cfg.Source(), "solana.program_id", "%v", err)
		}
		out.Solana = &SolanaReadback{
			ProgramID:      cfg.Solana.ProgramID,
			Commitment:     cfg.Solana.Commitment,
			ConfigAccount:  configAccount.Base58(),
			VaultAuthority: authority.Base58(),
		}
	}
	for i, asset := range cfg.Assets {
		field := fmt.Sprintf("%s[%d]", fieldAssets, i)
		capMsg := bundle.Caps[i]
		id, err := decodeField(cfg.Source(), field+".asset_id", asset.AssetID)
		if err != nil {
			return Readback{}, err
		}
		if capMsg.Asset != id {
			return Readback{}, refuse(cfg.Source(), field+".asset_id", "the bundle caps %s where the configuration lists %s",
				capMsg.Asset.Hex(), id.Hex())
		}
		entry := AssetReadback{
			Symbol:   asset.Symbol,
			Address:  asset.Address,
			AssetID:  id.Hex(),
			Decimals: asset.Decimals,
			PerTxCap: capMsg.MaxPerTx.String(),
			TotalCap: capMsg.MaxInFlight.String(),
			Denom:    types.Denom(cfg.ChainID, id),
		}
		if cfg.Solana == nil {
			entry.Address = strings.ToLower(asset.Address)
		} else {
			if id == sidioraAssetID {
				entry.Denom = types.SidioraDenom()
			}
			mint, err := vectors.Key(asset.Address)
			if err != nil {
				return Readback{}, refuse(cfg.Source(), field+".address", "%v", err)
			}
			account, _, err := vectors.FindProgramAddress(program, [][]byte{[]byte(solanaAssetSeed), mint[:]})
			if err != nil {
				return Readback{}, refuse(cfg.Source(), field+".address", "%v", err)
			}
			entry.Mint = "0x" + hex.EncodeToString(mint[:])
			entry.Account = account.Base58()
		}
		out.Assets = append(out.Assets, entry)
	}
	return out, nil
}

// marshalJSON writes a document that is not a module message, such as the
// read-back expectation, as indented JSON.
func marshalJSON(body any) ([]byte, error) {
	raw, err := json.MarshalIndent(body, "", "  ")
	if err != nil {
		return nil, err
	}
	return append(raw, '\n'), nil
}

// marshalBody writes msg as the protobuf JSON of an Any: its type URL under
// "@type" beside the message's own fields.
func marshalBody(msg sdk.Msg) ([]byte, error) {
	raw, err := messageCodec.MarshalInterfaceJSON(msg)
	if err != nil {
		return nil, err
	}
	var indented bytes.Buffer
	if err := json.Indent(&indented, raw, "", "  "); err != nil {
		return nil, err
	}
	indented.WriteByte('\n')
	return indented.Bytes(), nil
}

// marshalProposal writes a proposal as the protobuf JSON of the Any a
// submit-proposal transaction carries as its content: the proposal's type URL
// under "@type", its title and description, and every message as an Any of its
// own.
func marshalProposal(content *types.BridgeProposal) ([]byte, error) {
	if err := content.ValidateBasic(); err != nil {
		return nil, err
	}
	raw, err := messageCodec.MarshalInterfaceJSON(content)
	if err != nil {
		return nil, err
	}
	var indented bytes.Buffer
	if err := json.Indent(&indented, raw, "", "  "); err != nil {
		return nil, err
	}
	indented.WriteByte('\n')
	return indented.Bytes(), nil
}

// DecodeProposal reads one proposal back into the module's proposal content,
// resolving every message it carries under its type URL, with unknown fields
// forbidden, and refuses a proposal the handler would refuse.
func DecodeProposal(body []byte) (*types.BridgeProposal, error) {
	var content govtypes.Content
	if err := messageCodec.UnmarshalInterfaceJSON(body, &content); err != nil {
		return nil, err
	}
	proposal, ok := content.(*types.BridgeProposal)
	if !ok {
		return nil, fmt.Errorf("the proposal is a %T, not a %T", content, proposal)
	}
	if err := proposal.ValidateBasic(); err != nil {
		return nil, err
	}
	return proposal, nil
}

// DecodeBody reads one body back into the module message its type URL names,
// with unknown fields forbidden.
func DecodeBody(body []byte) (sdk.Msg, error) {
	var msg sdk.Msg
	if err := messageCodec.UnmarshalInterfaceJSON(body, &msg); err != nil {
		return nil, err
	}
	return msg, nil
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
// executes these messages for. The bodies travel in governance proposals, and
// a proposal executes only for the governance module account, so any other
// account is refused.
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
	if governance := types.DefaultAuthority(); trimmed != governance {
		return "", refuse(file, field, "%q is not the governance module account %s, the authority a governance proposal executes with",
			trimmed, governance)
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
