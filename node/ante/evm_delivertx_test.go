package ante_test

import (
	"encoding/hex"
	"math/big"
	"testing"

	"github.com/ethereum/go-ethereum/common"
	ethtypes "github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/crypto"
	evmante "github.com/sidiora-labs/paxeer-network/modules/evm/ante"
	"github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	evmtypes "github.com/sidiora-labs/paxeer-network/modules/evm/types"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types/ethtx"
	node "github.com/sidiora-labs/paxeer-network/node"
	"github.com/sidiora-labs/paxeer-network/node/ante"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

const (
	deliverFeeDenom = "usid"
	// deliverGasLimit at deliverGasPrice is 0.1 of the network coin.
	deliverGasLimit = uint64(100_000)
	deliverGasPrice = int64(1_000_000_000_000)
	deliverFeeWei   = int64(100_000_000_000_000_000)
	// deliverFeeConverted is 0.1 of the network coin at 3.114 Sidiora per Paxeer coin.
	deliverFeeConverted = int64(311_400)
	// deliverRefunded is the 79000 gas a plain transfer leaves unused at the same rate.
	deliverRefunded = int64(246_006)
	deliverFunded   = int64(1_000_000)
	deliverShort    = int64(1)
	deliverNative   = int64(1_000_000)
	deliverHeight   = int64(100)
)

type deliverPayer struct {
	app     *node.App
	ctx     sdk.Context
	k       *keeper.Keeper
	paxAddr sdk.AccAddress
	evmAddr common.Address
	msg     *evmtypes.MsgEVMTransaction
	tx      sdk.Tx
	hash    common.Hash
}

// newDeliverPayer funds an associated account with sidiora base units of
// Sidiora and native uhpx, turns the fee-token switch on with Sidiora at the
// initial governed rate, sets the account's fee denom to preference when it is
// not empty, and signs a plain transfer from it.
func newDeliverPayer(t *testing.T, app *node.App, preference string, sidiora, native int64) *deliverPayer {
	ctx, _ := app.GetContextForDeliverTx(nil).WithBlockHeight(deliverHeight).CacheContext()
	k := &app.EvmKeeper
	params := evmtypes.DefaultParams()
	params.FeeTokenEnabled = true
	params.AllowedFeeDenoms = []evmtypes.AllowedFeeDenom{{Denom: deliverFeeDenom, Rate: sdk.NewDec(evmtypes.InitialSidioraBaseUnitsPerPax), RateUpdateHeight: deliverHeight}}
	k.SetParams(ctx, params)

	privKey := testkeeper.MockPrivateKey()
	paxAddr, evmAddr := testkeeper.PrivateKeyToAddresses(privKey)
	k.SetAddressMapping(ctx, paxAddr, evmAddr)
	coins := sdk.NewCoins(sdk.NewInt64Coin(deliverFeeDenom, sidiora), sdk.NewInt64Coin(sdk.MustGetBaseDenom(), native))
	require.NoError(t, k.BankKeeper().MintCoins(ctx, evmtypes.ModuleName, coins))
	require.NoError(t, k.BankKeeper().SendCoinsFromModuleToAccount(ctx, evmtypes.ModuleName, paxAddr, coins))
	if preference != "" {
		require.NoError(t, k.SetAccountFeeDenom(ctx, evmAddr, preference))
	}

	key, err := crypto.HexToECDSA(hex.EncodeToString(privKey.Bytes()))
	require.NoError(t, err)
	to := common.HexToAddress("0x4567")
	signed, err := ethtypes.SignTx(ethtypes.NewTx(&ethtypes.LegacyTx{
		Nonce:    k.GetNonce(ctx, evmAddr),
		Gas:      deliverGasLimit,
		GasPrice: big.NewInt(deliverGasPrice),
		To:       &to,
		Value:    big.NewInt(0),
	}), ethtypes.LatestSignerForChainID(k.ChainID(ctx)), key)
	require.NoError(t, err)
	data, err := ethtx.NewLegacyTx(signed)
	require.NoError(t, err)
	msg, err := evmtypes.NewMsgEVMTransaction(data)
	require.NoError(t, err)
	builder := app.GetTxConfig().NewTxBuilder()
	require.NoError(t, builder.SetMsgs(msg))
	return &deliverPayer{app: app, ctx: ctx, k: k, paxAddr: paxAddr, evmAddr: evmAddr, msg: msg, tx: builder.GetTx(), hash: signed.Hash()}
}

func (p *deliverPayer) deliver() (sdk.Context, error) {
	return ante.EvmDeliverTxAnte(p.ctx, p.app.GetTxConfig(), p.tx, &p.app.UpgradeKeeper, p.k)
}

func (p *deliverPayer) feeToken() sdk.Int {
	return p.k.BankKeeper().GetBalance(p.ctx, p.paxAddr, deliverFeeDenom).Amount
}

// feeCheckOutcome runs the EVM fee check decorator the block execution path
// mirrors over a copy of the same state and reports the payer's fee-denom and
// network-coin balances after it and its error.
func (p *deliverPayer) feeCheckOutcome(t *testing.T) (sdk.Int, *big.Int, error) {
	ctx, _ := p.ctx.CacheContext()
	msg := *p.msg
	require.NoError(t, evmante.Preprocess(ctx, &msg, p.k.ChainID(ctx), false))
	builder := p.app.GetTxConfig().NewTxBuilder()
	require.NoError(t, builder.SetMsgs(&msg))
	_, err := evmante.NewEVMFeeCheckDecorator(p.k, &p.app.UpgradeKeeper).AnteHandle(ctx, builder.GetTx(), false, func(ctx sdk.Context, _ sdk.Tx, _ bool) (sdk.Context, error) { return ctx, nil })
	return p.k.BankKeeper().GetBalance(ctx, p.paxAddr, deliverFeeDenom).Amount, p.k.GetBalance(ctx, p.paxAddr), err
}

func TestEvmDeliverChargesFeeDenom(t *testing.T) {
	app := node.Setup(t, false, true, false)
	p := newDeliverPayer(t, app, deliverFeeDenom, deliverFunded, deliverNative)
	nativeBefore := p.k.GetBalance(p.ctx, p.paxAddr)
	checkToken, checkNative, checkErr := p.feeCheckOutcome(t)
	require.NoError(t, checkErr)

	_, err := p.deliver()
	require.NoError(t, err)

	require.Equal(t, sdk.NewInt(deliverFunded-deliverFeeConverted), p.feeToken())
	require.Equal(t, nativeBefore, p.k.GetBalance(p.ctx, p.paxAddr))
	require.Equal(t, checkToken, p.feeToken())
	require.Equal(t, checkNative, p.k.GetBalance(p.ctx, p.paxAddr))
	surplus, err := p.k.GetAnteSurplusSum(p.ctx)
	require.NoError(t, err)
	require.True(t, surplus.IsZero())
	charge, err := p.k.GetAnteFeeTokenCharge(p.ctx, p.hash)
	require.NoError(t, err)
	require.NotNil(t, charge)
	require.Equal(t, p.evmAddr, charge.Payer)
	require.Equal(t, deliverFeeDenom, charge.Denom)
	require.Equal(t, sdk.NewDec(evmtypes.InitialSidioraBaseUnitsPerPax), charge.Rate)

	// The message server returns the unused gas in the fee denom at the recorded rate.
	response, err := keeper.NewMsgServerImpl(p.k).EVMTransaction(sdk.WrapSDKContext(p.ctx), p.msg)
	require.NoError(t, err)
	require.Empty(t, response.VmError)
	require.Equal(t, uint64(21_000), response.GasUsed)
	require.Equal(t, sdk.NewInt(deliverFunded-deliverFeeConverted+deliverRefunded), p.feeToken())
	require.Equal(t, nativeBefore, p.k.GetBalance(p.ctx, p.paxAddr))
}

func TestEvmDeliverRefusesUncoveredFeeDenom(t *testing.T) {
	app := node.Setup(t, false, true, false)
	p := newDeliverPayer(t, app, deliverFeeDenom, deliverShort, deliverNative)
	nativeBefore := p.k.GetBalance(p.ctx, p.paxAddr)
	_, _, checkErr := p.feeCheckOutcome(t)
	require.ErrorIs(t, checkErr, sdkerrors.ErrInsufficientFunds)

	_, err := p.deliver()
	require.ErrorIs(t, err, sdkerrors.ErrInsufficientFunds)
	require.ErrorContains(t, err, "insufficient "+deliverFeeDenom+" for gas")
	require.Equal(t, checkErr.Error(), err.Error())

	require.Equal(t, sdk.NewInt(deliverShort), p.feeToken())
	require.Equal(t, nativeBefore, p.k.GetBalance(p.ctx, p.paxAddr))
	charge, err := p.k.GetAnteFeeTokenCharge(p.ctx, p.hash)
	require.NoError(t, err)
	require.Nil(t, charge)
}

func TestEvmDeliverChargesNetworkCoinWithoutFeeDenom(t *testing.T) {
	app := node.Setup(t, false, true, false)
	p := newDeliverPayer(t, app, "", deliverFunded, deliverNative)
	nativeBefore := p.k.GetBalance(p.ctx, p.paxAddr)
	checkToken, checkNative, checkErr := p.feeCheckOutcome(t)
	require.NoError(t, checkErr)

	_, err := p.deliver()
	require.NoError(t, err)

	require.Equal(t, new(big.Int).Sub(nativeBefore, big.NewInt(deliverFeeWei)), p.k.GetBalance(p.ctx, p.paxAddr))
	require.Equal(t, sdk.NewInt(deliverFunded), p.feeToken())
	require.Equal(t, checkToken, p.feeToken())
	require.Equal(t, checkNative, p.k.GetBalance(p.ctx, p.paxAddr))
	charge, err := p.k.GetAnteFeeTokenCharge(p.ctx, p.hash)
	require.NoError(t, err)
	require.Nil(t, charge)
}
