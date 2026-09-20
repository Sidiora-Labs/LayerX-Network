package precompiles_test

import (
	"testing"

	"github.com/ethereum/go-ethereum/core/vm"
	gigaprecompiles "github.com/sidiora-labs/paxeer-network/engine/executor/precompiles"
	gigautils "github.com/sidiora-labs/paxeer-network/engine/executor/utils"
	"github.com/stretchr/testify/require"
)

func TestSelfDestructAbortError(t *testing.T) {
	abortErr, ok := gigaprecompiles.ErrSelfDestructUnsupported.(vm.AbortError)
	require.True(t, ok, "ErrSelfDestructUnsupported must implement vm.AbortError")
	require.True(t, abortErr.IsAbortError())
	require.NotEmpty(t, gigaprecompiles.ErrSelfDestructUnsupported.Error())

	// ShouldExecutionAbort must recognize it so the giga executor falls back to v2.
	require.True(t, gigautils.ShouldExecutionAbort(gigaprecompiles.ErrSelfDestructUnsupported))
}
