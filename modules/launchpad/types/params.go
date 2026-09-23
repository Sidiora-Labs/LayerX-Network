package types

import (
	"fmt"
	"math/big"

	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// MaxProtocolFeeBps is ProtocolConfig.setProtocolFeeBps's bound (50%).
const MaxProtocolFeeBps = 5_000

// Params are KindleLaunch's ProtocolConfig fields. QuoteDenom is the bank
// denom that plays USDL. Params change only through UpdateParams, which
// accepts the governance module account alone.
type Params struct {
	QuoteDenom          string  `json:"quote_denom"`
	VirtualQuoteDefault sdk.Int `json:"virtual_quote_default"`
	VirtualTokenDefault sdk.Int `json:"virtual_token_default"`
	MinFeeBps           uint64  `json:"min_fee_bps"`
	MaxFeeBps           uint64  `json:"max_fee_bps"`
	BaseFeeBps          uint64  `json:"base_fee_bps"`
	ProtocolFeeBps      uint64  `json:"protocol_fee_bps"`
	FeeDecayRate        uint64  `json:"fee_decay_rate"`
	VolatilityWeight    uint64  `json:"volatility_weight"`
	ConcentrationWeight uint64  `json:"concentration_weight"`
	CreationFee         sdk.Int `json:"creation_fee"`
}

// DefaultQuoteDenom is the bank base denom of USDL on Paxeer.
const DefaultQuoteDenom = "uusdl"

// DefaultParams are ProtocolConfig.initialize's values.
func DefaultParams() Params {
	return Params{
		QuoteDenom:          DefaultQuoteDenom,
		VirtualQuoteDefault: sdk.NewInt(10_000_000_000),
		VirtualTokenDefault: sdk.NewInt(1_000_000_000_000_000),
		MinFeeBps:           10,
		MaxFeeBps:           300,
		BaseFeeBps:          30,
		ProtocolFeeBps:      1_000,
		FeeDecayRate:        500,
		VolatilityWeight:    100,
		ConcentrationWeight: 100,
		CreationFee:         sdk.NewInt(100_000_000),
	}
}

func (p Params) Validate() error {
	if err := sdk.ValidateDenom(p.QuoteDenom); err != nil {
		return fmt.Errorf("%w: quote denom: %v", ErrInvalidParams, err)
	}
	for _, value := range []sdk.Int{p.VirtualQuoteDefault, p.VirtualTokenDefault, p.CreationFee} {
		if value.IsNil() || value.IsNegative() || fits(value.BigInt()) != nil {
			return fmt.Errorf("%w: reserve defaults and creation fee must be uint256 integers", ErrInvalidParams)
		}
	}
	if !p.VirtualQuoteDefault.IsPositive() || !p.VirtualTokenDefault.IsPositive() {
		return fmt.Errorf("%w: virtual reserves must be positive", ErrInvalidParams)
	}
	if p.MinFeeBps > p.MaxFeeBps || p.MaxFeeBps >= BpsDenominator {
		return fmt.Errorf("%w: fee bounds must satisfy min <= max < 10000", ErrInvalidParams)
	}
	if p.BaseFeeBps > p.MaxFeeBps {
		return fmt.Errorf("%w: base fee above max fee", ErrInvalidParams)
	}
	if p.ProtocolFeeBps > MaxProtocolFeeBps {
		return fmt.Errorf("%w: protocol fee above 5000 bps", ErrInvalidParams)
	}
	return nil
}

// FeeInputs returns the ProtocolConfig side of FeeLib.calculateDynamicFee.
func (p Params) FeeInputs(poolAgeSeconds, volatility *big.Int) FeeInputs {
	return FeeInputs{
		BaseFee:             new(big.Int).SetUint64(p.BaseFeeBps),
		MinFee:              new(big.Int).SetUint64(p.MinFeeBps),
		MaxFee:              new(big.Int).SetUint64(p.MaxFeeBps),
		FeeDecayRate:        new(big.Int).SetUint64(p.FeeDecayRate),
		VolatilityWeight:    new(big.Int).SetUint64(p.VolatilityWeight),
		ConcentrationWeight: new(big.Int).SetUint64(p.ConcentrationWeight),
		PoolAgeSeconds:      poolAgeSeconds,
		Volatility:          volatility,
		// Concentration is 0 on chain, as in SidioraPool._calculateFee.
		TopHolderBps: new(big.Int),
	}
}
