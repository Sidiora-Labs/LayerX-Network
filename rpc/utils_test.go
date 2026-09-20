package evmrpc_test

import (
	"context"
	"testing"

	"github.com/sidiora-labs/paxeer-network/node"
	"github.com/sidiora-labs/paxeer-network/rpc"
	"github.com/stretchr/testify/require"
)

func TestCheckVersion(t *testing.T) {
	testApp := app.Setup(t, false, false, false)
	k := &testApp.EvmKeeper
	ctx := testApp.GetContextForDeliverTx([]byte{}).WithBlockHeight(1)
	testApp.Commit(context.Background()) // bump store version to 1
	require.Nil(t, evmrpc.CheckVersion(ctx, k))
	ctx = ctx.WithBlockHeight(2)
	require.NotNil(t, evmrpc.CheckVersion(ctx, k))
}
