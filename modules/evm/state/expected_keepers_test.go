package state_test

import (
	"github.com/sidiora-labs/paxeer-network/modules/evm/state"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
	"testing"
)

func TestFeeConversionKeeperInterface(t *testing.T) {
	var k state.EVMKeeper = &testkeeper.EVMTestApp.EvmKeeper
	fee, err := k.ConvertFeeToDenom(sdk.NewInt(1_000_000_000_000_000_000), sdk.NewDec(1_000_000), true)
	require.NoError(t, err)
	require.Equal(t, sdk.NewInt(1_000_000), fee)
	wei, err := k.ConvertFeeFromDenom(fee, sdk.NewDec(1_000_000), false)
	require.NoError(t, err)
	require.Equal(t, sdk.NewInt(1_000_000_000_000_000_000), wei)
}
