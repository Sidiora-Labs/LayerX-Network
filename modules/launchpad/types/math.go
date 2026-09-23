package types

import (
	"math/big"
)

// The functions below are integer ports of KindleLaunch's ReserveLib, FeeLib
// and SidioraMath. Every intermediate is an unbounded integer and every
// result that Solidity would hold in a uint256 is checked against 2^256-1, so
// an input that reverts there returns an error here and an input that
// succeeds there returns the same value here.

const (
	// BpsDenominator is the basis-point scale of every fee parameter.
	BpsDenominator = 10_000
	// SnapshotSlots is the length of the per-market price snapshot ring.
	SnapshotSlots = 8
)

var (
	maxUint256 = new(big.Int).Sub(new(big.Int).Lsh(big.NewInt(1), 256), big.NewInt(1))
	wad        = big.NewInt(1_000_000_000_000_000_000)
	micro      = big.NewInt(1_000_000)
	bps        = big.NewInt(BpsDenominator)
)

// MaxUint256 returns 2^256-1.
func MaxUint256() *big.Int { return new(big.Int).Set(maxUint256) }

func fits(value *big.Int) error {
	if value.Sign() < 0 || value.Cmp(maxUint256) > 0 {
		return ErrOverflow
	}
	return nil
}

func add(a, b *big.Int) (*big.Int, error) {
	out := new(big.Int).Add(a, b)
	return out, fits(out)
}

func mul(a, b *big.Int) (*big.Int, error) {
	out := new(big.Int).Mul(a, b)
	return out, fits(out)
}

// MulDiv is SidioraMath.mulDiv: floor(a*b/denominator) with a full-width
// product; it fails on a zero denominator or a result above 2^256-1.
func MulDiv(a, b, denominator *big.Int) (*big.Int, error) {
	if denominator.Sign() == 0 {
		return nil, ErrDivisionByZero
	}
	out := new(big.Int).Quo(new(big.Int).Mul(a, b), denominator)
	return out, fits(out)
}

// Sqrt is SidioraMath.sqrt, the Babylonian iteration from x.
func Sqrt(x *big.Int) *big.Int {
	if x.Sign() == 0 {
		return new(big.Int)
	}
	if x.Cmp(big.NewInt(3)) <= 0 {
		return big.NewInt(1)
	}
	z := new(big.Int).Set(x)
	y := new(big.Int).Add(new(big.Int).Rsh(x, 1), big.NewInt(1))
	for y.Cmp(z) < 0 {
		z.Set(y)
		y = new(big.Int).Rsh(new(big.Int).Add(new(big.Int).Quo(x, y), y), 1)
	}
	return z
}

// GetAmountOut is ReserveLib.getAmountOut:
// floor(reserveOut*amountIn / (reserveIn+amountIn)).
func GetAmountOut(reserveIn, reserveOut, amountIn *big.Int) (*big.Int, error) {
	if amountIn.Sign() == 0 {
		return nil, ErrInsufficientInput
	}
	if reserveIn.Sign() == 0 || reserveOut.Sign() == 0 {
		return nil, ErrInsufficientLiquidity
	}
	numerator, err := mul(reserveOut, amountIn)
	if err != nil {
		return nil, err
	}
	denominator, err := add(reserveIn, amountIn)
	if err != nil {
		return nil, err
	}
	return new(big.Int).Quo(numerator, denominator), nil
}

// GetAmountIn is ReserveLib.getAmountIn:
// floor(reserveIn*amountOut / (reserveOut-amountOut)) + 1.
func GetAmountIn(reserveIn, reserveOut, amountOut *big.Int) (*big.Int, error) {
	if amountOut.Sign() == 0 {
		return nil, ErrInsufficientInput
	}
	if reserveIn.Sign() == 0 || reserveOut.Sign() == 0 {
		return nil, ErrInsufficientLiquidity
	}
	if amountOut.Cmp(reserveOut) >= 0 {
		return nil, ErrInsufficientLiquidity
	}
	numerator, err := mul(reserveIn, amountOut)
	if err != nil {
		return nil, err
	}
	out := new(big.Int).Quo(numerator, new(big.Int).Sub(reserveOut, amountOut))
	return add(out, big.NewInt(1))
}

// GetPrice is ReserveLib.getPrice: effectiveQuote*1e18/tokenReserve.
func GetPrice(effectiveQuote, tokenReserve *big.Int) (*big.Int, error) {
	if tokenReserve.Sign() == 0 {
		return nil, ErrInsufficientLiquidity
	}
	return MulDiv(effectiveQuote, wad, tokenReserve)
}

// GetMarketCap is ReserveLib.getMarketCap: price*totalSupply/1e18.
func GetMarketCap(effectiveQuote, tokenReserve, totalSupply *big.Int) (*big.Int, error) {
	price, err := GetPrice(effectiveQuote, tokenReserve)
	if err != nil {
		return nil, err
	}
	return MulDiv(price, totalSupply, wad)
}

// CalculateAgeFactor is FeeLib.calculateAgeFactor:
// feeDecayRate / (1 + poolAgeSeconds/3600).
func CalculateAgeFactor(feeDecayRate, poolAgeSeconds *big.Int) (*big.Int, error) {
	hours := new(big.Int).Quo(poolAgeSeconds, big.NewInt(3600))
	divisor, err := add(big.NewInt(1), hours)
	if err != nil {
		return nil, err
	}
	return new(big.Int).Quo(feeDecayRate, divisor), nil
}

// CalculateVolatilityFactor is FeeLib.calculateVolatilityFactor:
// volatilityWeight*volatility/1e6.
func CalculateVolatilityFactor(volatilityWeight, volatility *big.Int) (*big.Int, error) {
	if volatility.Sign() == 0 {
		return new(big.Int), nil
	}
	return MulDiv(volatilityWeight, volatility, micro)
}

// CalculateConcentrationFactor is FeeLib.calculateConcentrationFactor:
// concentrationWeight*topHolderBps/10000.
func CalculateConcentrationFactor(concentrationWeight, topHolderBps *big.Int) (*big.Int, error) {
	if topHolderBps.Sign() == 0 {
		return new(big.Int), nil
	}
	product, err := mul(concentrationWeight, topHolderBps)
	if err != nil {
		return nil, err
	}
	return product.Quo(product, bps), nil
}

// FeeInputs are the arguments of FeeLib.calculateDynamicFee.
type FeeInputs struct {
	BaseFee             *big.Int
	MinFee              *big.Int
	MaxFee              *big.Int
	FeeDecayRate        *big.Int
	VolatilityWeight    *big.Int
	ConcentrationWeight *big.Int
	PoolAgeSeconds      *big.Int
	Volatility          *big.Int
	TopHolderBps        *big.Int
}

// CalculateDynamicFee is FeeLib.calculateDynamicFee: base + age + volatility
// + concentration, clamped to [minFee, maxFee].
func CalculateDynamicFee(in FeeInputs) (*big.Int, error) {
	age, err := CalculateAgeFactor(in.FeeDecayRate, in.PoolAgeSeconds)
	if err != nil {
		return nil, err
	}
	vol, err := CalculateVolatilityFactor(in.VolatilityWeight, in.Volatility)
	if err != nil {
		return nil, err
	}
	conc, err := CalculateConcentrationFactor(in.ConcentrationWeight, in.TopHolderBps)
	if err != nil {
		return nil, err
	}
	fee := new(big.Int).Set(in.BaseFee)
	for _, part := range []*big.Int{age, vol, conc} {
		if fee, err = add(fee, part); err != nil {
			return nil, err
		}
	}
	if fee.Cmp(in.MinFee) < 0 {
		fee.Set(in.MinFee)
	} else if fee.Cmp(in.MaxFee) > 0 {
		fee.Set(in.MaxFee)
	}
	return fee, nil
}

func absDiff(a, b *big.Int) *big.Int {
	if a.Cmp(b) > 0 {
		return new(big.Int).Sub(a, b)
	}
	return new(big.Int).Sub(b, a)
}

// CalculateVolatility is FeeLib.calculateVolatility over the first count
// slots of the snapshot array in index order: the standard deviation of the
// absolute consecutive price changes, scaled by 1e6.
func CalculateVolatility(snapshots [SnapshotSlots]*big.Int, count uint64) (*big.Int, error) {
	if count < 2 {
		return new(big.Int), nil
	}
	if count > SnapshotSlots {
		return nil, ErrOverflow
	}
	sumChanges := new(big.Int)
	changes := int64(0)
	var err error
	for i := uint64(1); i < count; i++ {
		if sumChanges, err = add(sumChanges, absDiff(snapshots[i], snapshots[i-1])); err != nil {
			return nil, err
		}
		changes++
	}
	meanChange := new(big.Int).Quo(sumChanges, big.NewInt(changes))
	sumSquaredDev := new(big.Int)
	for i := uint64(1); i < count; i++ {
		dev := absDiff(absDiff(snapshots[i], snapshots[i-1]), meanChange)
		squared, err := MulDiv(dev, dev, micro)
		if err != nil {
			return nil, err
		}
		if sumSquaredDev, err = add(sumSquaredDev, squared); err != nil {
			return nil, err
		}
	}
	variance := new(big.Int).Quo(sumSquaredDev, big.NewInt(changes))
	scaled, err := mul(variance, micro)
	if err != nil {
		return nil, err
	}
	return Sqrt(scaled), nil
}

// FeeAmount is the input-token fee of a swap: amountIn*feeBps/10000.
func FeeAmount(amountIn, feeBps *big.Int) (*big.Int, error) {
	product, err := mul(amountIn, feeBps)
	if err != nil {
		return nil, err
	}
	return product.Quo(product, bps), nil
}
