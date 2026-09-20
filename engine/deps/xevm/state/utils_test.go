package state_test

import (
	"math/big"
	"testing"

	"github.com/sidiora-labs/paxeer-network/engine/deps/xevm/state"
	"github.com/stretchr/testify/require"
)

func TestGetCoinbaseAddress(t *testing.T) {
	coinbaseAddr := state.GetCoinbaseAddress(1).String()
	require.Equal(t, coinbaseAddr, "pax1v4mx6hmrda5kucnpwdjsqqqqqqqqqqqpnepu9h")
}

func TestSplitUhpxWeiAmount(t *testing.T) {
	for _, test := range []struct {
		amt         *big.Int
		expectedPax *big.Int
		expectedWei *big.Int
	}{
		{
			amt:         big.NewInt(0),
			expectedPax: big.NewInt(0),
			expectedWei: big.NewInt(0),
		}, {
			amt:         big.NewInt(1),
			expectedPax: big.NewInt(0),
			expectedWei: big.NewInt(1),
		}, {
			amt:         big.NewInt(999_999_999_999),
			expectedPax: big.NewInt(0),
			expectedWei: big.NewInt(999_999_999_999),
		}, {
			amt:         big.NewInt(1_000_000_000_000),
			expectedPax: big.NewInt(1),
			expectedWei: big.NewInt(0),
		}, {
			amt:         big.NewInt(1_000_000_000_001),
			expectedPax: big.NewInt(1),
			expectedWei: big.NewInt(1),
		}, {
			amt:         big.NewInt(123_456_789_123_456_789),
			expectedPax: big.NewInt(123456),
			expectedWei: big.NewInt(789_123_456_789),
		},
	} {
		uhpx, wei := state.SplitUhpxWeiAmount(test.amt)
		require.Equal(t, test.expectedPax, uhpx.BigInt())
		require.Equal(t, test.expectedWei, wei.BigInt())
	}
}
