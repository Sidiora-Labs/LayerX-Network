package types

import (
	"fmt"

	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// AirdropEpochAmount is the amount snapshotted for one airdrop epoch.
type AirdropEpochAmount struct {
	Denom  string  `json:"denom"`
	Epoch  uint64  `json:"epoch"`
	Amount sdk.Int `json:"amount"`
}

// AirdropClaim records that holder claimed denom's airdrop for epoch.
type AirdropClaim struct {
	Denom  string `json:"denom"`
	Holder string `json:"holder"`
	Epoch  uint64 `json:"epoch"`
}

type GenesisState struct {
	Params              Params               `json:"params"`
	Markets             []Market             `json:"markets"`
	AirdropEpochs       []AirdropEpochAmount `json:"airdrop_epochs"`
	AirdropClaims       []AirdropClaim       `json:"airdrop_claims"`
	ProtocolFeesPending sdk.Int              `json:"protocol_fees_pending"`
}

func DefaultGenesis() *GenesisState {
	return &GenesisState{Params: DefaultParams(), Markets: []Market{}, AirdropEpochs: []AirdropEpochAmount{},
		AirdropClaims: []AirdropClaim{}, ProtocolFeesPending: sdk.ZeroInt()}
}

func (gs GenesisState) Validate() error {
	if err := gs.Params.Validate(); err != nil {
		return err
	}
	if gs.ProtocolFeesPending.IsNil() || gs.ProtocolFeesPending.IsNegative() {
		return fmt.Errorf("%w: protocol fees pending", ErrInvalidGenesis)
	}
	denoms := map[string]Market{}
	indexes := map[uint64]bool{}
	for _, market := range gs.Markets {
		if err := market.Validate(); err != nil {
			return err
		}
		if _, dup := denoms[market.Denom]; dup || indexes[market.Index] {
			return fmt.Errorf("%w: duplicate market %s", ErrInvalidGenesis, market.Denom)
		}
		denoms[market.Denom] = market
		indexes[market.Index] = true
	}
	for i := uint64(1); i <= uint64(len(gs.Markets)); i++ {
		if !indexes[i] {
			return fmt.Errorf("%w: market indexes must be 1..%d", ErrInvalidGenesis, len(gs.Markets))
		}
	}
	for _, epoch := range gs.AirdropEpochs {
		market, ok := denoms[epoch.Denom]
		if !ok || epoch.Epoch == 0 || epoch.Epoch > market.AirdropEpoch || epoch.Amount.IsNil() || epoch.Amount.IsNegative() {
			return fmt.Errorf("%w: airdrop epoch %s/%d", ErrInvalidGenesis, epoch.Denom, epoch.Epoch)
		}
	}
	for _, claim := range gs.AirdropClaims {
		market, ok := denoms[claim.Denom]
		if _, err := sdk.AccAddressFromBech32(claim.Holder); !ok || err != nil || claim.Epoch == 0 || claim.Epoch > market.AirdropEpoch {
			return fmt.Errorf("%w: airdrop claim %s/%s/%d", ErrInvalidGenesis, claim.Denom, claim.Holder, claim.Epoch)
		}
	}
	return nil
}
