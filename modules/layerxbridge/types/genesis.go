package types

import (
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
		if asset.Denom != Denom(asset.ChainID, asset.Asset) {
			return ErrInvalidGenesis.Wrapf("asset denom %s is not the derived denom", asset.Denom)
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
