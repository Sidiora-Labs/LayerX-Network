package keeper_test

import (
	"fmt"
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/tracing"
	ethtypes "github.com/ethereum/go-ethereum/core/types"
	"github.com/holiman/uint256"
	abci "github.com/sidiora-labs/paxeer-network/consensus/abci/types"
	"github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/evm/state"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	"github.com/sidiora-labs/paxeer-network/node"
	"github.com/sidiora-labs/paxeer-network/sdk/store/prefix"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
	authtypes "github.com/sidiora-labs/paxeer-network/sdk/x/auth/types"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

func TestEndBlock_NoReceiptForNonceMismatch(t *testing.T) {
	a := app.Setup(t, false, false, false)
	k := a.EvmKeeper
	ctx := a.GetContextForDeliverTx([]byte{}).WithBlockHeight(8)

	msg := mockEVMTransactionMessage(t)
	etx, _ := msg.AsTransaction()
	txHash := etx.Hash()

	k.BeginBlock(ctx)
	k.SetMsgs([]*types.MsgEVMTransaction{msg})
	k.SetTxResults([]*abci.ExecTxResult{{Code: 1, Log: "nonce mismatch"}})
	// No SetNonceBumped call — simulates a tx where startingNonce != txNonce,
	// so the nonce bump callback was never registered/executed.
	k.EndBlock(ctx, 0, 0)

	_, err := k.GetTransientReceipt(ctx, txHash, 0)
	require.Error(t, err, "should not create a receipt when nonce was not bumped")
}

func TestEndBlock_ReceiptCreatedWhenNonceBumped(t *testing.T) {
	a := app.Setup(t, false, false, false)
	k := a.EvmKeeper
	ctx := a.GetContextForDeliverTx([]byte{}).WithBlockHeight(8)

	msg := mockEVMTransactionMessage(t)
	etx, _ := msg.AsTransaction()
	txHash := etx.Hash()

	k.BeginBlock(ctx)
	k.SetMsgs([]*types.MsgEVMTransaction{msg})
	k.SetTxResults([]*abci.ExecTxResult{{Code: 1, Log: "some ante error"}})
	// Simulate that the nonce bump callback ran (startingNonce == txNonce).
	k.SetNonceBumped(ctx.WithTxIndex(0))
	k.EndBlock(ctx, 0, 0)

	receipt, err := k.GetTransientReceipt(ctx, txHash, 0)
	require.NoError(t, err, "should create a receipt when nonce was bumped")
	require.Equal(t, txHash.Hex(), receipt.TxHashHex)
	require.Equal(t, "some ante error", receipt.VmError)
	require.Equal(t, uint64(8), receipt.BlockNumber)
}

func TestAnteSurplusCorruptionFailsEndBlock(t *testing.T) {
	a := app.Setup(t, false, false, false)
	k := a.EvmKeeper
	ctx := a.GetContextForDeliverTx([]byte{}).WithBlockHeight(1)
	store := prefix.NewStore(ctx.TransientStore(a.GetTKey(types.TransientStoreKey)), types.AnteSurplusPrefix)
	store.Set(common.Hash{1}.Bytes(), []byte{0xff})

	_, err := k.GetAnteSurplusSum(ctx)
	require.Error(t, err)
	require.Panics(t, func() {
		k.EndBlock(ctx, 1, 0)
	})
}

func TestEndBlockFailsWhenSurplusCreditIsRejected(t *testing.T) {
	a := app.Setup(t, false, false, false)
	k := a.EvmKeeper
	ctx := a.GetContextForDeliverTx([]byte{}).WithBlockHeight(1)
	evmModule := k.AccountKeeper().GetModuleAddress(types.ModuleName)
	a.BankKeeper.RegisterRecipientChecker(func(_ sdk.Context, recipient sdk.AccAddress) bool {
		return !recipient.Equals(evmModule)
	})
	require.NoError(t, k.AddAnteSurplus(ctx, common.Hash{1}, sdk.NewInt(1_000_000_000_000)))

	require.Panics(t, func() {
		k.EndBlock(ctx, 1, 0)
	})
}

func TestEndBlockFailsWhenWeiSurplusCreditIsRejected(t *testing.T) {
	a := app.Setup(t, false, false, false)
	k := a.EvmKeeper
	ctx := a.GetContextForDeliverTx([]byte{}).WithBlockHeight(1)
	evmModule := k.AccountKeeper().GetModuleAddress(types.ModuleName)
	a.BankKeeper.RegisterRecipientChecker(func(_ sdk.Context, recipient sdk.AccAddress) bool {
		return !recipient.Equals(evmModule)
	})
	require.NoError(t, k.AddAnteSurplus(ctx, common.Hash{1}, sdk.OneInt()))

	require.Panics(t, func() {
		k.EndBlock(ctx, 1, 0)
	})
}

func TestEndBlockFailsWhenCoinbaseSweepIsRejected(t *testing.T) {
	a := app.Setup(t, false, false, false)
	k := a.EvmKeeper
	ctx := a.GetContextForDeliverTx([]byte{}).WithBlockHeight(1)
	msg := mockEVMTransactionMessage(t)
	k.SetMsgs([]*types.MsgEVMTransaction{msg})
	k.SetTxResults([]*abci.ExecTxResult{{Code: 0}})
	k.AppendToEvmTxDeferredInfo(ctx.WithTxIndex(0), ethtypes.Bloom{}, common.Hash{1}, sdk.ZeroInt())
	coinbase := state.GetCoinbaseAddress(0)
	require.NoError(t, a.BankKeeper.AddCoins(ctx, coinbase, sdk.NewCoins(sdk.NewCoin(k.GetBaseDenom(ctx), sdk.OneInt())), true))
	feeCollector := k.AccountKeeper().GetModuleAddress(authtypes.FeeCollectorName)
	a.BankKeeper.RegisterRecipientChecker(func(_ sdk.Context, recipient sdk.AccAddress) bool {
		return !recipient.Equals(feeCollector)
	})

	require.Panics(t, func() {
		k.EndBlock(ctx, 1, 0)
	})
}

const feeTokenSweepDenom = "usid"

var (
	feeTokenSweepSidioraReward = sdk.NewInt(1_868_400)
	feeTokenSweepNetworkReward = sdk.NewInt(600_000)
)

func setFeeTokenSweepParams(ctx sdk.Context, k *keeper.Keeper, enabled bool, distribute bool) {
	params := types.DefaultParams()
	params.AllowedFeeDenoms = []types.AllowedFeeDenom{
		{Denom: feeTokenSweepDenom, Rate: sdk.NewDec(types.InitialSidioraBaseUnitsPerPax), RateUpdateHeight: ctx.BlockHeight()},
	}
	params.FeeTokenEnabled = enabled
	params.FeeTokenDistribution = distribute
	k.SetParams(ctx, params)
}

// payFeeTokenSweepGas buys gas, refunds the unused part and rewards the coinbase for one
// transaction through the state DB, paying in Sidiora when the payer prefers it.
func payFeeTokenSweepGas(t *testing.T, a *app.App, ctx sdk.Context, txIndex int, paySidiora bool) {
	k := &a.EvmKeeper
	txCtx := ctx.WithTxIndex(txIndex)
	payer, evmAddr := testkeeper.MockAddressPair()
	k.SetAddressMapping(txCtx, payer, evmAddr)
	funds := sdk.NewCoins(sdk.NewInt64Coin(feeTokenSweepDenom, 10_000_000), sdk.NewInt64Coin(k.GetBaseDenom(txCtx), 10_000_000))
	require.NoError(t, a.BankKeeper.MintCoins(txCtx, types.ModuleName, funds))
	require.NoError(t, a.BankKeeper.SendCoinsFromModuleToAccount(txCtx, types.ModuleName, payer, funds))
	db := state.NewDBImpl(txCtx, k, false)
	if paySidiora {
		require.NoError(t, k.SetAccountFeeDenom(txCtx, evmAddr, feeTokenSweepDenom))
		charge, err := k.GetFeeTokenCharge(txCtx, evmAddr)
		require.NoError(t, err)
		require.NotNil(t, charge)
		db.SetFeeTokenCharge(charge, false)
	}
	coinbase, err := k.GetFeeCollectorAddress(txCtx)
	require.NoError(t, err)
	db.SubBalance(evmAddr, uint256.NewInt(1_000_000_000_000_000_000), tracing.BalanceDecreaseGasBuy)
	db.AddBalance(evmAddr, uint256.NewInt(400_000_000_000_000_000), tracing.BalanceIncreaseGasReturn)
	db.AddBalance(coinbase, uint256.NewInt(600_000_000_000_000_000), tracing.BalanceIncreaseRewardTransactionFee)
	require.NoError(t, db.Error())
	surplus, err := db.Finalize()
	require.NoError(t, err)
	require.True(t, surplus.IsZero())
	k.AppendToEvmTxDeferredInfo(txCtx, ethtypes.Bloom{}, common.Hash{byte(txIndex + 1)}, surplus)
}

// runFeeTokenSweepBlock records a block whose first transaction paid gas in Sidiora and whose
// second paid in the network coin, and returns the two transaction coinbase addresses.
func runFeeTokenSweepBlock(t *testing.T, a *app.App, ctx sdk.Context) []sdk.AccAddress {
	k := &a.EvmKeeper
	k.SetMsgs([]*types.MsgEVMTransaction{mockEVMTransactionMessage(t), mockEVMTransactionMessage(t)})
	k.SetTxResults([]*abci.ExecTxResult{{Code: 0}, {Code: 0}})
	payFeeTokenSweepGas(t, a, ctx, 0, true)
	payFeeTokenSweepGas(t, a, ctx, 1, false)
	coinbases := []sdk.AccAddress{state.GetCoinbaseAddress(0), state.GetCoinbaseAddress(1)}
	require.Equal(t, sdk.NewCoins(sdk.NewCoin(feeTokenSweepDenom, feeTokenSweepSidioraReward)), a.BankKeeper.GetAllBalances(ctx, coinbases[0]))
	require.Equal(t, sdk.NewCoins(sdk.NewCoin(k.GetBaseDenom(ctx), feeTokenSweepNetworkReward)), a.BankKeeper.GetAllBalances(ctx, coinbases[1]))
	return coinbases
}

func TestFeeTokenSweepMovesCoinbaseFeesToCollector(t *testing.T) {
	a := app.Setup(t, false, false, false)
	k := &a.EvmKeeper
	ctx := a.GetContextForDeliverTx([]byte{}).WithBlockHeight(1)
	setFeeTokenSweepParams(ctx, k, true, false)
	collector := k.AccountKeeper().GetModuleAddress(authtypes.FeeCollectorName)
	require.True(t, a.BankKeeper.GetAllBalances(ctx, collector).Empty())
	coinbases := runFeeTokenSweepBlock(t, a, ctx)

	k.EndBlock(ctx, 1, 0)

	for _, coinbase := range coinbases {
		require.True(t, a.BankKeeper.GetAllBalances(ctx, coinbase).Empty())
		require.True(t, a.BankKeeper.GetWeiBalance(ctx, coinbase).IsZero())
	}
	require.Equal(t, sdk.NewCoins(
		sdk.NewCoin(feeTokenSweepDenom, feeTokenSweepSidioraReward),
		sdk.NewCoin(k.GetBaseDenom(ctx), feeTokenSweepNetworkReward),
	), a.BankKeeper.GetAllBalances(ctx, collector))
}

func TestFeeTokenSweepSwitchedOffLeavesFeeTokensAtCoinbase(t *testing.T) {
	a := app.Setup(t, false, false, false)
	k := &a.EvmKeeper
	ctx := a.GetContextForDeliverTx([]byte{}).WithBlockHeight(1)
	setFeeTokenSweepParams(ctx, k, true, false)
	coinbases := runFeeTokenSweepBlock(t, a, ctx)
	setFeeTokenSweepParams(ctx, k, false, false)
	collector := k.AccountKeeper().GetModuleAddress(authtypes.FeeCollectorName)

	k.EndBlock(ctx, 1, 0)

	require.Equal(t, sdk.NewCoins(sdk.NewCoin(feeTokenSweepDenom, feeTokenSweepSidioraReward)), a.BankKeeper.GetAllBalances(ctx, coinbases[0]))
	require.True(t, a.BankKeeper.GetAllBalances(ctx, coinbases[1]).Empty())
	require.Equal(t, sdk.NewCoins(sdk.NewCoin(k.GetBaseDenom(ctx), feeTokenSweepNetworkReward)), a.BankKeeper.GetAllBalances(ctx, collector))
}

func TestFeeTokenSweepRejectedFailsEndBlock(t *testing.T) {
	a := app.Setup(t, false, false, false)
	k := &a.EvmKeeper
	ctx := a.GetContextForDeliverTx([]byte{}).WithBlockHeight(1)
	setFeeTokenSweepParams(ctx, k, true, false)
	k.SetMsgs([]*types.MsgEVMTransaction{mockEVMTransactionMessage(t)})
	k.SetTxResults([]*abci.ExecTxResult{{Code: 0}})
	payFeeTokenSweepGas(t, a, ctx, 0, true)
	coinbase := state.GetCoinbaseAddress(0)
	require.Equal(t, sdk.NewCoins(sdk.NewCoin(feeTokenSweepDenom, feeTokenSweepSidioraReward)), a.BankKeeper.GetAllBalances(ctx, coinbase))
	collector := k.AccountKeeper().GetModuleAddress(authtypes.FeeCollectorName)
	a.BankKeeper.RegisterRecipientChecker(func(_ sdk.Context, recipient sdk.AccAddress) bool {
		return !recipient.Equals(collector)
	})

	recovered := func() (value interface{}) {
		defer func() { value = recover() }()
		k.EndBlock(ctx, 1, 0)
		return nil
	}()

	require.NotNil(t, recovered)
	require.ErrorIs(t, recovered.(error), sdkerrors.ErrInvalidRecipient)
	require.Contains(t, fmt.Sprint(recovered), "end block: sweep coinbase fee tokens 1868400usid from "+coinbase.String())
}
