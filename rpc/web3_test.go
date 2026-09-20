package evmrpc_test

import (
	"testing"

	"github.com/sidiora-labs/paxeer-network/rpc"
	"github.com/stretchr/testify/require"
)

func TestClientVersion(t *testing.T) {
	w := evmrpc.Web3API{}
	require.NotEmpty(t, w.ClientVersion())
}
