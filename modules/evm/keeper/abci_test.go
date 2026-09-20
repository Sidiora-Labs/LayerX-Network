package keeper_test

import (
	"testing"

	"github.com/ethereum/go-ethereum/common"
	ethtypes "github.com/ethereum/go-ethereum/core/types"
	abci "github.com/sidiora-labs/paxeer-network/consensus/abci/types"
	"github.com/sidiora-labs/paxeer-network/modules/evm/state"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	"github.com/sidiora-labs/paxeer-network/node"
	"github.com/sidiora-labs/paxeer-network/sdk/store/prefix"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	authtypes "github.com/sidiora-labs/paxeer-network/sdk/x/auth/types"
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
