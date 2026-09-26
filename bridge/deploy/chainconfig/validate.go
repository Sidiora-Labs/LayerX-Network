package chainconfig

import (
	"bytes"
	"encoding/hex"
	"fmt"
	"path/filepath"
	"regexp"
	"strings"
)

// Validate is the schema and the consistency of a chain configuration. It is
// what makes a configuration trustworthy: a file cannot claim a chain id, a
// native coin, an asset id or a cap of its own, and every refusal names the
// file and the field it refuses.
//
// Validate accepts the placeholder owner, attestors and program id a committed
// configuration carries, because a placeholder is a well-formed value the owner
// has still to fill in. RequireDeployable is what refuses those: a tool that is
// about to act on a chain calls it and stops.
func (c *ChainConfig) Validate() error {
	chain, ok := ChainByName(c.Chain)
	if !ok {
		return c.refuse("chain", "%q is not a bridge chain", c.Chain)
	}
	if err := c.validateLocation(chain); err != nil {
		return err
	}
	if c.Kind != chain.Kind {
		return c.refuse("kind", "%s is %q, not %q", chain.Name, chain.Kind, c.Kind)
	}
	if c.ChainID != chain.ID {
		return c.refuse("chain_id", "%s is chain %d, not %d", chain.Name, chain.ID, c.ChainID)
	}
	if c.Native.Symbol != chain.NativeSymbol {
		return c.refuse("native.symbol", "the native coin of %s is %s, not %q", chain.Name, chain.NativeSymbol, c.Native.Symbol)
	}
	if c.Native.Decimals != chain.NativeDecimals {
		return c.refuse("native.decimals", "%s has %d decimals on %s, not %d", chain.NativeSymbol, chain.NativeDecimals, chain.Name, c.Native.Decimals)
	}
	if err := c.validateEnvironment(chain); err != nil {
		return err
	}
	if err := c.validateOwner(chain); err != nil {
		return err
	}
	if err := c.validateAttestors(); err != nil {
		return err
	}
	if c.Threshold == 0 {
		return c.refuse("threshold", "a threshold of zero would accept an unsigned attestation")
	}
	if int(c.Threshold) > len(c.Attestors) {
		return c.refuse("threshold", "%d is above the %d attestors of the set", c.Threshold, len(c.Attestors))
	}
	if c.FinalityDepth == 0 {
		return c.refuse("finality_depth", "a finality depth of zero would admit an unconfirmed deposit")
	}
	if err := c.validateBigBlocks(chain); err != nil {
		return err
	}
	if err := c.validateSolanaSection(chain); err != nil {
		return err
	}
	return c.validateAssets(chain)
}

// RequireDeployable is the gate a tool calls before it acts on a chain. It runs
// Validate and then refuses every value the owner has still to fill in: the
// placeholder owner, a placeholder attestor, the placeholder Solana program id
// and, on the chain whose deployment needs big blocks, a requirement the owner
// has not acknowledged. There is no default and no substitution.
func (c *ChainConfig) RequireDeployable() error {
	if err := c.Validate(); err != nil {
		return err
	}
	if IsPlaceholder(c.Owner) {
		return c.refuse("owner", "%s is a placeholder; fill in the owner that holds the deployment", c.Owner)
	}
	for index, attestor := range c.Attestors {
		if IsPlaceholder(attestor) {
			return c.refuse(fmt.Sprintf("attestors[%d]", index), "%s is a placeholder; fill in the attestor set", attestor)
		}
	}
	if c.Solana != nil && IsPlaceholder(c.Solana.ProgramID) {
		return c.refuse("solana.program_id", "%s is a placeholder; deploy the program and record its id", c.Solana.ProgramID)
	}
	if c.BigBlocks != nil && c.BigBlocks.Required && !c.BigBlocks.Acknowledged {
		return c.refuse("big_blocks.acknowledged", "%s", c.BigBlocks.Requirement)
	}
	return nil
}

var (
	environmentName  = regexp.MustCompile(`^[A-Z][A-Z0-9]*(_[A-Z0-9]+)*$`)
	placeholderLabel = regexp.MustCompile(`^[a-z0-9]+(-[a-z0-9]+)*$`)
	hexAddress       = regexp.MustCompile(`^0x[0-9a-fA-F]{40}$`)
	commitments      = []string{"confirmed", "finalized"}
)

func (c *ChainConfig) validateLocation(chain Chain) error {
	if c.path == "" {
		return nil
	}
	if filepath.Base(c.path) != configFileName {
		return c.refuse("chain", "a chain configuration is named %s", configFileName)
	}
	if directory := filepath.Base(filepath.Dir(c.path)); directory != chain.Name {
		return c.refuse("chain", "the file lies in %q but names the chain %q", directory, c.Chain)
	}
	return nil
}

func (c *ChainConfig) validateEnvironment(chain Chain) error {
	required := map[string]string{
		"environment.rpc_url":    c.Environment.RPCURL,
		"environment.deploy_key": c.Environment.DeployKey,
	}
	switch chain.Kind {
	case KindEVM:
		required["environment.explorer_key"] = c.Environment.ExplorerKey
		if c.Environment.ToolchainBin != "" {
			return c.refuse("environment.toolchain_bin", "only Solana deploys through a pinned toolchain directory")
		}
	case KindSolana:
		required["environment.toolchain_bin"] = c.Environment.ToolchainBin
		if c.Environment.ExplorerKey != "" {
			return c.refuse("environment.explorer_key", "nothing in the bridge verifies a Solana program through an explorer key")
		}
	}
	for field, name := range required {
		if name == "" {
			return c.refuse(field, "the name of the environment variable the value arrives in is required")
		}
		if !environmentName.MatchString(name) {
			return c.refuse(field, "%q is not an upper snake case environment variable name", name)
		}
	}
	return nil
}

func (c *ChainConfig) validateOwner(chain Chain) error {
	if IsPlaceholder(c.Owner) {
		return c.validatePlaceholder("owner", c.Owner)
	}
	switch chain.Kind {
	case KindEVM:
		return c.validateEVMAddress("owner", c.Owner, false)
	default:
		return c.validateSolanaKey("owner", c.Owner)
	}
}

func (c *ChainConfig) validateAttestors() error {
	if len(c.Attestors) == 0 {
		return c.refuse("attestors", "the attestor set is empty")
	}
	if len(c.Attestors) > MaxAttestors {
		return c.refuse("attestors", "the bridge carries at most %d attestors, not %d", MaxAttestors, len(c.Attestors))
	}
	placeholders := 0
	for _, attestor := range c.Attestors {
		if IsPlaceholder(attestor) {
			placeholders++
		}
	}
	if placeholders != 0 && placeholders != len(c.Attestors) {
		return c.refuse("attestors", "%d of the %d attestors are placeholders; a half-filled set would bring the chain up with the wrong signers", placeholders, len(c.Attestors))
	}
	seen := make(map[string]struct{}, len(c.Attestors))
	var previous []byte
	for index, attestor := range c.Attestors {
		field := fmt.Sprintf("attestors[%d]", index)
		key := strings.ToLower(attestor)
		if _, duplicate := seen[key]; duplicate {
			return c.refuse(field, "%s appears twice in the attestor set", attestor)
		}
		seen[key] = struct{}{}
		if placeholders != 0 {
			if err := c.validatePlaceholder(field, attestor); err != nil {
				return err
			}
			continue
		}
		if err := c.validateEVMAddress(field, attestor, false); err != nil {
			return err
		}
		raw, err := hex.DecodeString(strings.TrimPrefix(strings.ToLower(attestor), "0x"))
		if err != nil {
			return c.refuse(field, "%s is not a 20-byte address: %v", attestor, err)
		}
		if previous != nil && bytes.Compare(raw, previous) <= 0 {
			return c.refuse(field, "%s does not follow %s; the attestor set is strictly ascending, as every destination verifies it", attestor, c.Attestors[index-1])
		}
		previous = raw
	}
	return nil
}

func (c *ChainConfig) validateBigBlocks(chain Chain) error {
	if chain.Name != BigBlockChain {
		if c.BigBlocks != nil {
			return c.refuse("big_blocks", "only %s needs the deploying account switched to big blocks", BigBlockChain)
		}
		return nil
	}
	if c.BigBlocks == nil {
		return c.refuse("big_blocks", "%s must record the big-block requirement its deployment needs", BigBlockChain)
	}
	if !c.BigBlocks.Required {
		return c.refuse("big_blocks.required", "a contract deployment on %s does not fit in a small block", BigBlockChain)
	}
	if strings.TrimSpace(c.BigBlocks.Requirement) == "" {
		return c.refuse("big_blocks.requirement", "the requirement the owner acknowledges must be written down")
	}
	return nil
}

func (c *ChainConfig) validateSolanaSection(chain Chain) error {
	if chain.Kind != KindSolana {
		if c.Solana != nil {
			return c.refuse("solana", "only the Solana chain holds custody in a program")
		}
		return nil
	}
	if c.Solana == nil {
		return c.refuse("solana", "the Solana configuration carries the program id and the commitment")
	}
	if IsPlaceholder(c.Solana.ProgramID) {
		if err := c.validatePlaceholder("solana.program_id", c.Solana.ProgramID); err != nil {
			return err
		}
	} else if err := c.validateSolanaKey("solana.program_id", c.Solana.ProgramID); err != nil {
		return err
	}
	for _, commitment := range commitments {
		if c.Solana.Commitment == commitment {
			return nil
		}
	}
	return c.refuse("solana.commitment", "%q is not one of %s; a lower commitment would read state that can still be dropped", c.Solana.Commitment, strings.Join(commitments, ", "))
}

func (c *ChainConfig) validateAssets(chain Chain) error {
	if len(c.Assets) == 0 {
		return c.refuse("assets", "the asset list is empty; the chain's native coin is always registered")
	}
	symbols := make(map[string]struct{}, len(c.Assets))
	addresses := make(map[string]struct{}, len(c.Assets))
	for index, asset := range c.Assets {
		field := fmt.Sprintf("assets[%d]", index)
		if strings.TrimSpace(asset.Symbol) == "" {
			return c.refuse(field+".symbol", "an asset carries the symbol it is known by")
		}
		if _, duplicate := symbols[asset.Symbol]; duplicate {
			return c.refuse(field+".symbol", "%s appears twice in the asset list", asset.Symbol)
		}
		symbols[asset.Symbol] = struct{}{}
		address := strings.ToLower(asset.Address)
		if _, duplicate := addresses[address]; duplicate {
			return c.refuse(field+".address", "%s appears twice in the asset list", asset.Address)
		}
		addresses[address] = struct{}{}
		if asset.Decimals == 0 || asset.Decimals > 18 {
			return c.refuse(field+".decimals", "%d is not between 1 and 18", asset.Decimals)
		}
		if err := c.validateCaps(field, asset); err != nil {
			return err
		}
		switch chain.Kind {
		case KindEVM:
			if err := c.validateEVMAsset(chain, field, index, asset); err != nil {
				return err
			}
		case KindSolana:
			if err := c.validateSolanaAsset(chain, field, index, asset); err != nil {
				return err
			}
		}
	}
	return nil
}

func (c *ChainConfig) validateCaps(field string, asset Asset) error {
	perTx, err := asset.PerTx()
	if err != nil {
		return c.refuse(field+".per_tx_cap", "%v", err)
	}
	if perTx.Sign() == 0 {
		return c.refuse(field+".per_tx_cap", "a per-transaction cap of zero would refuse every deposit of %s", asset.Symbol)
	}
	total, err := asset.Total()
	if err != nil {
		return c.refuse(field+".total_cap", "%v", err)
	}
	if total.Sign() == 0 {
		return c.refuse(field+".total_cap", "a total cap of zero leaves %s unregistered", asset.Symbol)
	}
	if perTx.Cmp(total) > 0 {
		return c.refuse(field+".per_tx_cap", "%s is above the total cap %s of %s", asset.PerTxCap, asset.TotalCap, asset.Symbol)
	}
	return nil
}

func (c *ChainConfig) validateEVMAsset(chain Chain, field string, index int, asset Asset) error {
	native := strings.EqualFold(asset.Address, NativeEVMAsset)
	if index == 0 {
		if !native {
			return c.refuse(field+".address", "the first asset of %s is its native coin %s at %s, not %s", chain.Name, chain.NativeSymbol, NativeEVMAsset, asset.Address)
		}
		if asset.Symbol != chain.NativeSymbol {
			return c.refuse(field+".symbol", "the native coin of %s is %s, not %s", chain.Name, chain.NativeSymbol, asset.Symbol)
		}
		if asset.Decimals != chain.NativeDecimals {
			return c.refuse(field+".decimals", "%s has %d decimals on %s, not %d", chain.NativeSymbol, chain.NativeDecimals, chain.Name, asset.Decimals)
		}
	} else if native {
		return c.refuse(field+".address", "the native coin is the first asset of %s and appears once", chain.Name)
	}
	if err := c.validateEVMAddress(field+".address", asset.Address, index == 0); err != nil {
		return err
	}
	if !strings.EqualFold(asset.AssetID, asset.Address) {
		return c.refuse(field+".asset_id", "an asset on %s enters the digests as its own address %s, not %s", chain.Name, asset.Address, asset.AssetID)
	}
	return nil
}

func (c *ChainConfig) validateSolanaAsset(chain Chain, field string, index int, asset Asset) error {
	wrapped := asset.Address == WrappedSolMint
	if index == 0 {
		if !wrapped {
			return c.refuse(field+".address", "the first asset of %s is the wrapped SOL mint %s, not %s", chain.Name, WrappedSolMint, asset.Address)
		}
		if asset.Symbol != chain.NativeSymbol {
			return c.refuse(field+".symbol", "the native coin of %s is %s, not %s", chain.Name, chain.NativeSymbol, asset.Symbol)
		}
		if asset.Decimals != chain.NativeDecimals {
			return c.refuse(field+".decimals", "wrapped %s has %d decimals, not %d", chain.NativeSymbol, chain.NativeDecimals, asset.Decimals)
		}
	} else if wrapped {
		return c.refuse(field+".address", "the wrapped SOL mint is the first asset of %s and appears once", chain.Name)
	}
	if err := c.validateSolanaKey(field+".address", asset.Address); err != nil {
		return err
	}
	if err := c.validateEVMAddress(field+".asset_id", asset.AssetID, false); err != nil {
		return err
	}
	if asset.Address == SidioraMint {
		if !strings.EqualFold(asset.AssetID, SidioraAssetID) {
			return c.refuse(field+".asset_id", "Sidiora's mint enters the digests as %s, the id the chain fixed for it through EnsureSidioraDenom, not %s", SidioraAssetID, asset.AssetID)
		}
		if asset.Decimals != 6 {
			return c.refuse(field+".decimals", "Sidiora has six decimals on both of its chains, not %d", asset.Decimals)
		}
		return nil
	}
	if strings.EqualFold(asset.AssetID, SidioraAssetID) {
		return c.refuse(field+".asset_id", "%s is the id the chain fixed for Sidiora's mint %s, not for %s", SidioraAssetID, SidioraMint, asset.Address)
	}
	handle, err := SolanaHandle(asset.Address)
	if err != nil {
		return c.refuse(field+".address", "%v", err)
	}
	if !strings.EqualFold(asset.AssetID, handle) {
		return c.refuse(field+".asset_id", "the handle of %s is %s, not %s", asset.Address, handle, asset.AssetID)
	}
	return nil
}

func (c *ChainConfig) validatePlaceholder(field, value string) error {
	label := strings.TrimPrefix(value, PlaceholderPrefix)
	if !placeholderLabel.MatchString(label) {
		return c.refuse(field, "%q is not a placeholder of the shape %s<label>", value, PlaceholderPrefix)
	}
	return nil
}

// validateEVMAddress accepts a 0x-prefixed 20-byte address. The zero address is
// refused everywhere but where it is the asset id of a chain's native coin,
// which is what address(0) means to the vault.
func (c *ChainConfig) validateEVMAddress(field, value string, allowZero bool) error {
	if !hexAddress.MatchString(value) {
		return c.refuse(field, "%q is not a 0x-prefixed 20-byte address", value)
	}
	raw, err := hex.DecodeString(strings.TrimPrefix(strings.ToLower(value), "0x"))
	if err != nil {
		return c.refuse(field, "%q is not a 20-byte address: %v", value, err)
	}
	if !allowZero && bytes.Equal(raw, make([]byte, len(raw))) {
		return c.refuse(field, "the zero address is not a value this configuration can carry")
	}
	return nil
}

func (c *ChainConfig) validateSolanaKey(field, value string) error {
	raw, err := DecodeBase58(value)
	if err != nil {
		return c.refuse(field, "%v", err)
	}
	if len(raw) != 32 {
		return c.refuse(field, "a Solana key is 32 bytes, %q decodes to %d", value, len(raw))
	}
	if value == SolanaZeroPubkey {
		return c.refuse(field, "the zero pubkey is not %s", strings.TrimSuffix(field, ".address"))
	}
	return nil
}

func (c *ChainConfig) refuse(field, format string, args ...any) error {
	return fmt.Errorf("%s: %s: %s", c.source(), field, fmt.Sprintf(format, args...))
}

func (c *ChainConfig) source() string {
	if c.path != "" {
		return c.path
	}
	return "chain " + c.Chain
}
