package types_test

import (
	"math/big"
	"testing"

	"github.com/sidiora-labs/paxeer-network/modules/launchpad/types"
	"github.com/stretchr/testify/require"
)

var (
	virtualQuote = big.NewInt(10_000_000_000)
	virtualToken = big.NewInt(1_000_000_000_000_000)
)

func n(value int64) *big.Int { return big.NewInt(value) }

func sameInt(t *testing.T, want, got *big.Int, msgAndArgs ...interface{}) {
	t.Helper()
	require.Equal(t, want.String(), got.String(), msgAndArgs...)
}

func units(whole int64) *big.Int { return new(big.Int).Mul(n(whole), n(1_000_000)) }

func must(t *testing.T) func(*big.Int, error) *big.Int {
	return func(value *big.Int, err error) *big.Int {
		t.Helper()
		require.NoError(t, err)
		return value
	}
}

func feeInputs(base, minFee, maxFee, decay, volWeight, concWeight, age, volatility, topHolder int64) types.FeeInputs {
	return types.FeeInputs{BaseFee: n(base), MinFee: n(minFee), MaxFee: n(maxFee), FeeDecayRate: n(decay),
		VolatilityWeight: n(volWeight), ConcentrationWeight: n(concWeight), PoolAgeSeconds: n(age),
		Volatility: n(volatility), TopHolderBps: n(topHolder)}
}

func TestFeeLibAgeFactor(t *testing.T) {
	for _, tc := range []struct{ decay, age, want int64 }{
		{500, 0, 500}, {500, 3600, 250}, {500, 86400, 20}, {500, 499 * 3600, 1}, {500, 1000 * 3600, 0}, {0, 3600, 0},
	} {
		sameInt(t, n(tc.want), must(t)(types.CalculateAgeFactor(n(tc.decay), n(tc.age))), "age %d", tc.age)
	}
}

func TestFeeLibVolatilityAndConcentrationFactors(t *testing.T) {
	sameInt(t, n(0), must(t)(types.CalculateVolatilityFactor(n(100), n(0))))
	sameInt(t, n(50), must(t)(types.CalculateVolatilityFactor(n(100), n(500_000))))
	sameInt(t, n(200), must(t)(types.CalculateVolatilityFactor(n(100), n(2_000_000))))
	sameInt(t, n(0), must(t)(types.CalculateVolatilityFactor(n(0), n(1_000_000))))
	sameInt(t, n(0), must(t)(types.CalculateConcentrationFactor(n(100), n(0))))
	sameInt(t, n(50), must(t)(types.CalculateConcentrationFactor(n(100), n(5000))))
	sameInt(t, n(100), must(t)(types.CalculateConcentrationFactor(n(100), n(10000))))
}

func TestFeeLibDynamicFee(t *testing.T) {
	for name, tc := range map[string]struct {
		in   types.FeeInputs
		want int64
	}{
		"mature pool is base fee":     {feeInputs(30, 10, 300, 500, 100, 100, 1000*3600, 0, 0), 30},
		"new pool clamps to max":      {feeInputs(30, 10, 300, 500, 100, 100, 0, 0, 0), 300},
		"clamps to min":               {feeInputs(0, 10, 300, 0, 0, 0, 86400*365, 0, 0), 10},
		"everything maxed":            {feeInputs(30, 10, 300, 500, 100, 100, 0, 3_000_000, 10000), 300},
		"combines all factors":        {feeInputs(30, 10, 300, 500, 100, 100, 86400, 100_000, 2000), 80},
		"moderate age and volatility": {feeInputs(30, 10, 300, 500, 100, 100, 3600, 300_000, 0), 300},
	} {
		sameInt(t, n(tc.want), must(t)(types.CalculateDynamicFee(tc.in)), name)
	}
}

func ring(values ...int64) [types.SnapshotSlots]*big.Int {
	var out [types.SnapshotSlots]*big.Int
	for i := range out {
		out[i] = new(big.Int)
		if i < len(values) {
			out[i] = n(values[i])
		}
	}
	return out
}

func TestFeeLibVolatility(t *testing.T) {
	single := ring(100_000_000)
	sameInt(t, n(0), must(t)(types.CalculateVolatility(single, 1)))
	sameInt(t, n(0), must(t)(types.CalculateVolatility(single, 0)))
	sameInt(t, n(0), must(t)(types.CalculateVolatility(ring(10, 10, 10, 10), 4)))
	moving := must(t)(types.CalculateVolatility(ring(100_000, 120_000, 90_000, 150_000), 4))
	sameInt(t, n(16970), moving)
	small := must(t)(types.CalculateVolatility(ring(100_000, 101_000, 99_000, 102_000), 4))
	big := must(t)(types.CalculateVolatility(ring(100_000, 200_000, 50_000, 300_000), 4))
	sameInt(t, n(0), small)
	sameInt(t, n(62353), big)
	require.Equal(t, 1, big.Cmp(small))
	_, err := types.CalculateVolatility(ring(), types.SnapshotSlots+1)
	require.ErrorIs(t, err, types.ErrOverflow)
}

func TestReserveLibAmountOut(t *testing.T) {
	buy := must(t)(types.GetAmountOut(virtualQuote, virtualToken, units(100)))
	sameInt(t, n(9_900_990_099_009), buy)
	require.Equal(t, 1, buy.Cmp(units(9_900_000)))
	require.Equal(t, -1, buy.Cmp(units(10_000_000)))

	sell := must(t)(types.GetAmountOut(virtualToken, virtualQuote, units(10_000_000)))
	sameInt(t, n(99_009_900), sell)

	_, err := types.GetAmountOut(virtualQuote, virtualToken, n(0))
	require.ErrorIs(t, err, types.ErrInsufficientInput)
	_, err = types.GetAmountOut(n(0), virtualToken, units(100))
	require.ErrorIs(t, err, types.ErrInsufficientLiquidity)
	_, err = types.GetAmountOut(virtualQuote, n(0), units(100))
	require.ErrorIs(t, err, types.ErrInsufficientLiquidity)

	huge := must(t)(types.GetAmountOut(virtualQuote, virtualToken, units(999_999_999_999)))
	require.Equal(t, -1, huge.Cmp(virtualToken))

	out10 := must(t)(types.GetAmountOut(virtualQuote, virtualToken, units(10)))
	out200 := must(t)(types.GetAmountOut(virtualQuote, virtualToken, units(200)))
	out1000 := must(t)(types.GetAmountOut(virtualQuote, virtualToken, units(1000)))
	require.Equal(t, 1, buy.Cmp(out10))
	require.Equal(t, 1, out1000.Cmp(buy))
	require.Equal(t, -1, out200.Cmp(new(big.Int).Mul(buy, n(2))))

	in500 := units(500)
	out500 := must(t)(types.GetAmountOut(virtualQuote, virtualToken, in500))
	kBefore := new(big.Int).Mul(virtualQuote, virtualToken)
	kAfter := new(big.Int).Mul(new(big.Int).Add(virtualQuote, in500), new(big.Int).Sub(virtualToken, out500))
	sameInt(t, n(6_500_000_000), new(big.Int).Sub(kAfter, kBefore))

	sameInt(t, n(99_999), must(t)(types.GetAmountOut(virtualQuote, virtualToken, n(1))))
}

func TestReserveLibAmountInAndRoundTrip(t *testing.T) {
	desired := units(10_000_000)
	in := must(t)(types.GetAmountIn(virtualQuote, virtualToken, desired))
	sameInt(t, n(101_010_102), in)
	require.GreaterOrEqual(t, must(t)(types.GetAmountOut(virtualQuote, virtualToken, in)).Cmp(desired), 0)
	_, err := types.GetAmountIn(virtualQuote, virtualToken, n(0))
	require.ErrorIs(t, err, types.ErrInsufficientInput)
	_, err = types.GetAmountIn(virtualQuote, virtualToken, virtualToken)
	require.ErrorIs(t, err, types.ErrInsufficientLiquidity)
	_, err = types.GetAmountIn(n(0), virtualToken, units(100))
	require.ErrorIs(t, err, types.ErrInsufficientLiquidity)

	buyIn := units(100)
	received := must(t)(types.GetAmountOut(virtualQuote, virtualToken, buyIn))
	returned := must(t)(types.GetAmountOut(new(big.Int).Sub(virtualToken, received), new(big.Int).Add(virtualQuote, buyIn), received))
	sameInt(t, n(99_999_999), returned)
	require.Equal(t, -1, returned.Cmp(buyIn))
}

func TestReserveLibPriceAndMarketCap(t *testing.T) {
	sameInt(t, n(10_000_000_000_000), must(t)(types.GetPrice(virtualQuote, virtualToken)))
	require.Equal(t, units(10_000), must(t)(types.GetMarketCap(virtualQuote, virtualToken, virtualToken)))
	_, err := types.GetPrice(virtualQuote, n(0))
	require.ErrorIs(t, err, types.ErrInsufficientLiquidity)

	buyIn := units(1000)
	out := must(t)(types.GetAmountOut(virtualQuote, virtualToken, buyIn))
	quoteAfter, tokenAfter := new(big.Int).Add(virtualQuote, buyIn), new(big.Int).Sub(virtualToken, out)
	before := must(t)(types.GetPrice(virtualQuote, virtualToken))
	after := must(t)(types.GetPrice(quoteAfter, tokenAfter))
	require.Equal(t, 1, after.Cmp(before))
	sold := new(big.Int).Quo(out, n(2))
	quoteBack := must(t)(types.GetAmountOut(tokenAfter, quoteAfter, sold))
	afterSell := must(t)(types.GetPrice(new(big.Int).Sub(quoteAfter, quoteBack), new(big.Int).Add(tokenAfter, sold)))
	require.Equal(t, -1, afterSell.Cmp(after))

	bigBuy := units(5000)
	outBig := must(t)(types.GetAmountOut(virtualQuote, virtualToken, bigBuy))
	capAfter := must(t)(types.GetMarketCap(new(big.Int).Add(virtualQuote, bigBuy), new(big.Int).Sub(virtualToken, outBig), virtualToken))
	require.Equal(t, 1, capAfter.Cmp(units(10_000)))
}

func TestFeeAmount(t *testing.T) {
	require.Equal(t, units(3), must(t)(types.FeeAmount(units(100), n(300))))
	require.Equal(t, units(10), must(t)(types.FeeAmount(n(333_333_334), n(300))))
	require.Equal(t, units(1), must(t)(types.FeeAmount(units(10), n(1000))))
}

func TestParamsValidate(t *testing.T) {
	require.NoError(t, types.DefaultParams().Validate())
	bad := types.DefaultParams()
	bad.ProtocolFeeBps = types.MaxProtocolFeeBps + 1
	require.ErrorIs(t, bad.Validate(), types.ErrInvalidParams)
	bad = types.DefaultParams()
	bad.MinFeeBps = bad.MaxFeeBps + 1
	require.ErrorIs(t, bad.Validate(), types.ErrInvalidParams)
	bad = types.DefaultParams()
	bad.VirtualQuoteDefault = bad.VirtualQuoteDefault.SubRaw(10_000_000_000)
	require.ErrorIs(t, bad.Validate(), types.ErrInvalidParams)
}
