package keeper_test

import (
	"testing"

	"github.com/sidiora-labs/paxeer-network/modules/epoch/keeper"
	keepertest "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

func TestSetupMsgServer(t *testing.T) {
	k, _ := keepertest.EpochKeeper(t)
	msg := keeper.NewMsgServerImpl(*k)
	require.NotNil(t, msg)
}
