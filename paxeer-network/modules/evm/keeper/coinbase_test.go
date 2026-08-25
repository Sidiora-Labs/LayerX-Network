package keeper_test

import (
	"testing"

	"github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

func TestGetFeeCollectorAddress(t *testing.T) {
	k := &testkeeper.EVMTestApp.EvmKeeper
	ctx := testkeeper.EVMTestApp.GetContextForDeliverTx([]byte{})
	addr, err := k.GetFeeCollectorAddress(ctx)
	require.Nil(t, err)
	expected := k.GetEVMAddressOrDefault(ctx, k.AccountKeeper().GetModuleAddress("fee_collector"))
	require.Equal(t, expected.Hex(), addr.Hex())
}

func TestGetCoinbaseAddress(t *testing.T) {
	require.Equal(t, "0x27F7B8B8B5A4e71E8E9aA671f4e4031E3773303F", keeper.GetCoinbaseAddress().Hex())
}
