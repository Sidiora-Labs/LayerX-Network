package state_test

import (
	"github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	"testing"
	"time"

	"github.com/ethereum/go-ethereum/core/tracing"
	"github.com/holiman/uint256"
	"github.com/sidiora-labs/paxeer-network/modules/evm/state"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

func TestAddBalance(t *testing.T) {
	k := &testkeeper.EVMTestApp.EvmKeeper
	ctx := testkeeper.EVMTestApp.GetContextForDeliverTx([]byte{}).WithBlockTime(time.Now())
	db := state.NewDBImpl(ctx, k, false)
	paxAddr, evmAddr := testkeeper.MockAddressPair()
	require.Equal(t, uint256.NewInt(0), db.GetBalance(evmAddr))
	db.AddBalance(evmAddr, uint256.NewInt(0), tracing.BalanceChangeUnspecified)

	// set association
	k.SetAddressMapping(db.Ctx(), paxAddr, evmAddr)
	require.Equal(t, uint256.NewInt(0), db.GetBalance(evmAddr))
	db.AddBalance(evmAddr, uint256.NewInt(10000000000000), tracing.BalanceChangeUnspecified)
	require.Nil(t, db.Err())
	require.Equal(t, db.GetBalance(evmAddr), uint256.NewInt(10000000000000))
}

func TestSubBalance(t *testing.T) {
	k := &testkeeper.EVMTestApp.EvmKeeper
	ctx := testkeeper.EVMTestApp.GetContextForDeliverTx([]byte{}).WithBlockTime(time.Now())
	db := state.NewDBImpl(ctx, k, false)
	paxAddr, evmAddr := testkeeper.MockAddressPair()
	require.Equal(t, uint256.NewInt(0), db.GetBalance(evmAddr))
	db.SubBalance(evmAddr, uint256.NewInt(0), tracing.BalanceChangeUnspecified)

	// set association
	k.SetAddressMapping(db.Ctx(), paxAddr, evmAddr)
	require.Equal(t, uint256.NewInt(0), db.GetBalance(evmAddr))
	amt := sdk.NewCoins(sdk.NewCoin(k.GetBaseDenom(ctx), sdk.NewInt(20)))
	k.BankKeeper().MintCoins(db.Ctx(), types.ModuleName, amt)
	k.BankKeeper().SendCoinsFromModuleToAccount(db.Ctx(), types.ModuleName, paxAddr, amt)
	db.SubBalance(evmAddr, uint256.NewInt(10000000000000), tracing.BalanceChangeUnspecified)
	require.Nil(t, db.Err())
	require.Equal(t, db.GetBalance(evmAddr), uint256.NewInt(10000000000000))
}

func TestSetBalance(t *testing.T) {
	k := &testkeeper.EVMTestApp.EvmKeeper
	ctx := testkeeper.EVMTestApp.GetContextForDeliverTx([]byte{}).WithBlockTime(time.Now())
	db := state.NewDBImpl(ctx, k, true)
	_, evmAddr := testkeeper.MockAddressPair()
	db.SetBalance(evmAddr, uint256.NewInt(10000000000000), tracing.BalanceChangeUnspecified)
	require.Equal(t, uint256.NewInt(10000000000000), db.GetBalance(evmAddr))

	paxAddr2, evmAddr2 := testkeeper.MockAddressPair()
	k.SetAddressMapping(db.Ctx(), paxAddr2, evmAddr2)
	db.SetBalance(evmAddr2, uint256.NewInt(10000000000000), tracing.BalanceChangeUnspecified)
	require.Equal(t, uint256.NewInt(10000000000000), db.GetBalance(evmAddr2))
}

func TestSurplus(t *testing.T) {
	k := &testkeeper.EVMTestApp.EvmKeeper
	ctx := testkeeper.EVMTestApp.GetContextForDeliverTx([]byte{}).WithBlockTime(time.Now())
	_, evmAddr := testkeeper.MockAddressPair()

	// test negative uhpx surplus negative wei surplus
	db := state.NewDBImpl(ctx, k, false)
	db.AddBalance(evmAddr, uint256.NewInt(1_000_000_000_001), tracing.BalanceChangeUnspecified)
	_, err := db.Finalize()
	require.Nil(t, err)

	// test negative uhpx surplus positive wei surplus (negative total)
	db = state.NewDBImpl(ctx, k, false)
	db.AddBalance(evmAddr, uint256.NewInt(1_000_000_000_000), tracing.BalanceChangeUnspecified)
	db.SubBalance(evmAddr, uint256.NewInt(1), tracing.BalanceChangeUnspecified)
	_, err = db.Finalize()
	require.Nil(t, err)

	// test negative uhpx surplus positive wei surplus (positive total)
	db = state.NewDBImpl(ctx, k, false)
	db.AddBalance(evmAddr, uint256.NewInt(1_000_000_000_000), tracing.BalanceChangeUnspecified)
	db.SubBalance(evmAddr, uint256.NewInt(2), tracing.BalanceChangeUnspecified)
	db.SubBalance(evmAddr, uint256.NewInt(999_999_999_999), tracing.BalanceChangeUnspecified)
	surplus, err := db.Finalize()
	require.Nil(t, err)
	require.Equal(t, sdk.OneInt(), surplus)

	// test positive uhpx surplus negative wei surplus (negative total)
	db = state.NewDBImpl(ctx, k, false)
	db.SubBalance(evmAddr, uint256.NewInt(1_000_000_000_000), tracing.BalanceChangeUnspecified)
	db.AddBalance(evmAddr, uint256.NewInt(2), tracing.BalanceChangeUnspecified)
	db.AddBalance(evmAddr, uint256.NewInt(999_999_999_999), tracing.BalanceChangeUnspecified)
	_, err = db.Finalize()
	require.Nil(t, err)

	// test positive uhpx surplus negative wei surplus (positive total)
	db = state.NewDBImpl(ctx, k, false)
	db.SubBalance(evmAddr, uint256.NewInt(1_000_000_000_000), tracing.BalanceChangeUnspecified)
	db.AddBalance(evmAddr, uint256.NewInt(999_999_999_999), tracing.BalanceChangeUnspecified)
	surplus, err = db.Finalize()
	require.Nil(t, err)
	require.Equal(t, sdk.OneInt(), surplus)

	// test snapshots
	db = state.NewDBImpl(ctx, k, false)
	db.SubBalance(evmAddr, uint256.NewInt(1_000_000_000_000), tracing.BalanceChangeUnspecified)
	db.AddBalance(evmAddr, uint256.NewInt(999_999_999_999), tracing.BalanceChangeUnspecified)
	db.Snapshot()
	db.SubBalance(evmAddr, uint256.NewInt(1_000_000_000_000), tracing.BalanceChangeUnspecified)
	db.AddBalance(evmAddr, uint256.NewInt(999_999_999_999), tracing.BalanceChangeUnspecified)
	db.Snapshot()
	db.SubBalance(evmAddr, uint256.NewInt(1_000_000_000_000), tracing.BalanceChangeUnspecified)
	db.AddBalance(evmAddr, uint256.NewInt(999_999_999_999), tracing.BalanceChangeUnspecified)
	surplus, err = db.Finalize()
	require.Nil(t, err)
	require.Equal(t, sdk.NewInt(3), surplus)
}

func TestFeeTokenBalanceMovements(t *testing.T) {
	app := testkeeper.EVMTestApp
	k := &app.EvmKeeper
	ctx, _ := app.GetContextForDeliverTx(nil).CacheContext()
	payer, evmAddr := testkeeper.MockAddressPair()
	k.SetAddressMapping(ctx, payer, evmAddr)
	coins := sdk.NewCoins(sdk.NewInt64Coin("usid", 10_000_000), sdk.NewInt64Coin("uhpx", 1_000_000))
	require.NoError(t, k.BankKeeper().MintCoins(ctx, types.ModuleName, coins))
	require.NoError(t, k.BankKeeper().SendCoinsFromModuleToAccount(ctx, types.ModuleName, payer, coins))
	db := state.NewDBImpl(ctx, k, false)
	charge := &state.FeeTokenCharge{Payer: evmAddr, Denom: "usid", Rate: sdk.NewDec(2_000_000)}
	db.SetFeeTokenCharge(charge, false)
	require.Equal(t, uint256.NewInt(6_000_000_000_000_000_000), db.GetBalance(evmAddr))
	before := k.BankKeeper().GetBalance(db.Ctx(), payer, "usid").Amount
	db.SubBalance(evmAddr, uint256.NewInt(1_000_000_000_000_000_001), tracing.BalanceDecreaseGasBuy)
	require.NoError(t, db.Error())
	afterDebit := k.BankKeeper().GetBalance(db.Ctx(), payer, "usid").Amount
	require.Equal(t, sdk.NewInt(7_999_999), afterDebit)
	debited := before.Sub(afterDebit)
	require.Equal(t, sdk.NewInt(2_000_001), debited)
	require.Equal(t, uint256.NewInt(1_000_000_000_000_000_000), db.GetBalance(evmAddr))
	db.AddBalance(evmAddr, uint256.NewInt(500_000_000_000_000_001), tracing.BalanceIncreaseGasReturn)
	afterRefund := k.BankKeeper().GetBalance(db.Ctx(), payer, "usid").Amount
	require.Equal(t, sdk.NewInt(9_000_000), afterRefund)
	refunded := afterRefund.Sub(afterDebit)
	require.Equal(t, sdk.NewInt(1_000_001), refunded)
	coinbase, err := k.GetFeeCollectorAddress(ctx)
	require.NoError(t, err)
	db.AddBalance(coinbase, uint256.NewInt(500_000_000_000_000_001), tracing.BalanceIncreaseRewardTransactionFee)
	require.NoError(t, db.Error())
	rewarded := k.BankKeeper().GetBalance(db.Ctx(), state.GetCoinbaseAddress(ctx.TxIndex()), "usid").Amount
	require.Equal(t, sdk.NewInt(1_000_000), rewarded)
	require.True(t, refunded.Add(rewarded).LTE(debited))
	require.Equal(t, uint256.NewInt(1_000_000_000_000_000_000), db.GetBalance(evmAddr))
	surplus, err := db.Finalize()
	require.NoError(t, err)
	require.True(t, surplus.IsZero())
}

func TestFeeTokenOtherBalanceReasons(t *testing.T) {
	app := testkeeper.EVMTestApp
	k := &app.EvmKeeper
	for _, reason := range []tracing.BalanceChangeReason{tracing.BalanceChangeUnspecified, tracing.BalanceChangeTransfer, tracing.BalanceIncreaseRewardTransactionFee} {
		ctx, _ := app.GetContextForDeliverTx(nil).CacheContext()
		payer, evmAddr := testkeeper.MockAddressPair()
		k.SetAddressMapping(ctx, payer, evmAddr)
		db := state.NewDBImpl(ctx, k, false)
		db.SetFeeTokenCharge(&state.FeeTokenCharge{Payer: evmAddr, Denom: "usid", Rate: sdk.NewDec(2_000_000)}, true)
		db.AddBalance(evmAddr, uint256.NewInt(123), reason)
		require.NoError(t, db.Error())
		require.Equal(t, uint256.NewInt(123), db.GetBalance(evmAddr))
		db.SubBalance(evmAddr, uint256.NewInt(23), reason)
		require.NoError(t, db.Error())
		require.Equal(t, uint256.NewInt(100), db.GetBalance(evmAddr))
		require.True(t, k.BankKeeper().GetBalance(db.Ctx(), payer, "usid").Amount.IsZero())
		surplus, err := db.Finalize()
		require.NoError(t, err)
		require.Equal(t, sdk.NewInt(-100), surplus)
	}
}

func TestFeeTokenConversionErrorInState(t *testing.T) {
	app := testkeeper.EVMTestApp
	k := &app.EvmKeeper
	for _, debit := range []bool{true, false} {
		ctx, _ := app.GetContextForDeliverTx(nil).CacheContext()
		_, payer := testkeeper.MockAddressPair()
		db := state.NewDBImpl(ctx, k, false)
		db.SetFeeTokenCharge(&state.FeeTokenCharge{Payer: payer, Denom: "usid", Rate: sdk.ZeroDec()}, true)
		if debit {
			db.SubBalance(payer, uint256.NewInt(1), tracing.BalanceDecreaseGasBuy)
		} else {
			db.AddBalance(payer, uint256.NewInt(1), tracing.BalanceIncreaseGasReturn)
		}
		require.ErrorIs(t, db.Error(), keeper.ErrFeeTokenRateInvalid)
		_, err := db.Finalize()
		require.ErrorIs(t, err, keeper.ErrFeeTokenRateInvalid)
	}
}
