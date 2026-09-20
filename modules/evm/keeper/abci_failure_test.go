package keeper

import (
	"errors"
	"testing"

	"github.com/stretchr/testify/require"
)

func TestFailEndBlockOnReceiptPersistenceError(t *testing.T) {
	require.PanicsWithError(t, "end block: persist failed receipt: storage unavailable", func() {
		failEndBlockOnError("persist failed receipt", errors.New("storage unavailable"))
	})
}
