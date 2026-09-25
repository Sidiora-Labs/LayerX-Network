package state_test

import (
	"github.com/ethereum/go-ethereum/core/tracing"
	"github.com/holiman/uint256"
	"github.com/sidiora-labs/paxeer-network/modules/evm/state"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
	"testing"
)

func TestFeeTokenChargeSurvivesStateCopy(t *testing.T) {
	app := testkeeper.EVMTestApp
	k := &app.EvmKeeper
	ctx, _ := app.GetContextForDeliverTx(nil).CacheContext()
	payer, address := testkeeper.MockAddressPair()
	k.SetAddressMapping(ctx, payer, address)
	coins := sdk.NewCoins(sdk.NewInt64Coin("usid", 1_000_000))
	require.NoError(t, k.BankKeeper().MintCoins(ctx, types.ModuleName, coins))
	require.NoError(t, k.BankKeeper().SendCoinsFromModuleToAccount(ctx, types.ModuleName, payer, coins))
	db := state.NewDBImpl(ctx, k, false)
	db.SetFeeTokenCharge(&state.FeeTokenCharge{Payer: address, Denom: "usid", Rate: sdk.NewDec(1_000_000)}, false)
	copied := db.Copy().(*state.DBImpl)
	require.Equal(t, uint256.NewInt(1_000_000_000_000_000_000), copied.GetBalance(address))
	copied.SubBalance(address, uint256.NewInt(100_000_000_000_000_000), tracing.BalanceDecreaseGasBuy)
	require.NoError(t, copied.Error())
	require.True(t, copied.GetBalance(address).IsZero())
	require.True(t, copied.Copy().GetBalance(address).IsZero())
	require.Equal(t, uint256.NewInt(1_000_000_000_000_000_000), db.GetBalance(address))
	db.ResetForTracer()
	require.True(t, db.GetBalance(address).IsZero())
}
