package indexer

import (
	"math"
	"testing"
	"time"

	"github.com/stretchr/testify/require"
)

func TestQueryRangeRejectsUnsupportedAndOverflowingBounds(t *testing.T) {
	t.Parallel()

	_, err := (QueryRange{LowerBound: "remote-string"}).LowerBoundValue()
	require.Error(t, err)

	_, err = (QueryRange{LowerBound: int64(math.MaxInt64)}).LowerBoundValue()
	require.Error(t, err)

	_, err = (QueryRange{UpperBound: int64(math.MinInt64)}).UpperBoundValue()
	require.Error(t, err)

	lower, err := (QueryRange{LowerBound: time.Unix(12, 0)}).LowerBoundValue()
	require.NoError(t, err)
	require.Equal(t, int64(13), lower)

	upper, err := (QueryRange{UpperBound: int64(12)}).UpperBoundValue()
	require.NoError(t, err)
	require.Equal(t, int64(11), upper)
}
