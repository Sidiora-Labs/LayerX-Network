package types

import (
	custodytypes "github.com/sidiora-labs/paxeer-network/modules/layerxcustody/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
)

// MaxMarkets bounds the params list scanned per intent.
const MaxMarkets = 256

func DefaultParams() Params { return Params{} }

// Validate checks one market listing.
func (m Market) Validate() error {
	if _, err := custodytypes.ParseNonZeroHash32(m.MarketId); err != nil {
		return sdkerrors.Wrapf(ErrInvalidMarket, "market id: %s", err)
	}
	if _, err := custodytypes.ParseNonZeroHash32(m.MarginAssetId); err != nil {
		return sdkerrors.Wrapf(ErrInvalidMarket, "margin asset id: %s", err)
	}
	return nil
}

func (p Params) Validate() error {
	if p.Authority != "" {
		if _, err := sdk.AccAddressFromBech32(p.Authority); err != nil {
			return sdkerrors.Wrapf(ErrInvalidParams, "authority: %s", err)
		}
	}
	if len(p.Markets) > MaxMarkets {
		return sdkerrors.Wrap(ErrInvalidParams, "too many markets")
	}
	seen := map[string]bool{}
	for _, market := range p.Markets {
		if err := market.Validate(); err != nil {
			return err
		}
		if seen[market.MarketId] {
			return sdkerrors.Wrapf(ErrInvalidParams, "market %s is listed twice", market.MarketId)
		}
		seen[market.MarketId] = true
	}
	return nil
}

// Market returns the listing of marketID.
func (p Params) Market(marketID string) (Market, bool) {
	for _, market := range p.Markets {
		if market.MarketId == marketID {
			return market, true
		}
	}
	return Market{}, false
}

// MarginAsset reports whether assetID margins at least one enabled market.
func (p Params) MarginAsset(assetID string) bool {
	for _, market := range p.Markets {
		if market.Enabled && market.MarginAssetId == assetID {
			return true
		}
	}
	return false
}
