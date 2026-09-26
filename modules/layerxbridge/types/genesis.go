package types

import (
	"github.com/ethereum/go-ethereum/common"
	authtypes "github.com/sidiora-labs/paxeer-network/sdk/x/auth/types"
	govtypes "github.com/sidiora-labs/paxeer-network/sdk/x/gov/types"
)

// GenesisState is the whole bridge state. The default registers nothing,
// sets no attestors and no caps, and is not paused: the bridge ships dormant
// and only a governance registration brings a chain up.
type GenesisState struct {
	Params         Params          `json:"params"`
	Paused         bool            `json:"paused"`
	Chains         []Chain         `json:"chains"`
	Attestors      AttestorSet     `json:"attestors"`
	Assets         []BridgedAsset  `json:"assets"`
	Caps           []Cap           `json:"caps"`
	InFlight       []InFlight      `json:"in_flight"`
	Nullifiers     []Nullifier     `json:"nullifiers"`
	OutboundNonces []OutboundNonce `json:"outbound_nonces"`
}

// DefaultAuthority is the gov module account.
func DefaultAuthority() string {
	return authtypes.NewModuleAddress(govtypes.ModuleName).String()
}

func DefaultGenesis() *GenesisState {
	return &GenesisState{Params: DefaultParams(DefaultAuthority())}
}

// IsSidioraPair reports whether (chainID, asset) is the pair
// RegisterSidioraPair records: Sidiora's remote address on its foreign home.
func IsSidioraPair(chainID uint64, asset Address20) bool {
	return chainID == SidioraHomeChainID && asset == Address20(common.HexToAddress(SidioraRemoteAddress))
}

// AssetDenom is the one denom a (chain, asset) pair may carry: the usid denom
// for Sidiora's pair and the chain-derived denom for every other pair, so
// neither form can stand in for the other.
func AssetDenom(chainID uint64, asset Address20) string {
	if IsSidioraPair(chainID, asset) {
		return SidioraDenom()
	}
	return Denom(chainID, asset)
}

func (g GenesisState) Validate() error {
	if err := g.Params.Validate(); err != nil {
		return err
	}
	chains := map[uint64]bool{}
	for _, chain := range g.Chains {
		if err := chain.Validate(); err != nil {
			return ErrInvalidGenesis.Wrap(err.Error())
		}
		if chains[chain.ChainID] {
			return ErrInvalidGenesis.Wrapf("duplicate chain %d", chain.ChainID)
		}
		chains[chain.ChainID] = true
	}
	if err := g.Attestors.Validate(); err != nil {
		return ErrInvalidGenesis.Wrap(err.Error())
	}
	denoms := map[string]bool{}
	for _, asset := range g.Assets {
		if !chains[asset.ChainID] {
			return ErrInvalidGenesis.Wrapf("asset of unregistered chain %d", asset.ChainID)
		}
		if asset.Denom != AssetDenom(asset.ChainID, asset.Asset) {
			return ErrInvalidGenesis.Wrapf("asset denom %s is not the registered denom of chain %d", asset.Denom, asset.ChainID)
		}
		if denoms[asset.Denom] {
			return ErrInvalidGenesis.Wrapf("duplicate asset %s", asset.Denom)
		}
		denoms[asset.Denom] = true
	}
	capped := map[string]bool{}
	for _, c := range g.Caps {
		if err := c.Validate(); err != nil {
			return ErrInvalidGenesis.Wrap(err.Error())
		}
		if !denoms[c.Denom] || capped[c.Denom] {
			return ErrInvalidGenesis.Wrapf("cap of unknown or duplicate denom %s", c.Denom)
		}
		capped[c.Denom] = true
	}
	flying := map[string]bool{}
	for _, entry := range g.InFlight {
		if !denoms[entry.Denom] || flying[entry.Denom] || entry.Amount.IsNil() || entry.Amount.IsNegative() {
			return ErrInvalidGenesis.Wrapf("in-flight entry of %s", entry.Denom)
		}
		flying[entry.Denom] = true
	}
	nullifiers := map[Nullifier]bool{}
	for _, nullifier := range g.Nullifiers {
		if !chains[nullifier.ChainID] || nullifiers[nullifier] {
			return ErrInvalidGenesis.Wrapf("nullifier of unknown chain or duplicate (%d)", nullifier.ChainID)
		}
		nullifiers[nullifier] = true
	}
	nonces := map[uint64]bool{}
	for _, nonce := range g.OutboundNonces {
		if !chains[nonce.ChainID] || nonces[nonce.ChainID] {
			return ErrInvalidGenesis.Wrapf("outbound nonce of unknown or duplicate chain %d", nonce.ChainID)
		}
		nonces[nonce.ChainID] = true
	}
	return nil
}
