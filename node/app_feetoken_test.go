package app_test

import (
	"math/big"
	"testing"
	"time"

	"github.com/ethereum/go-ethereum/common"
	ethtypes "github.com/ethereum/go-ethereum/core/types"
	abci "github.com/sidiora-labs/paxeer-network/consensus/abci/types"
	"github.com/sidiora-labs/paxeer-network/modules/evm/config"
	evmkeeper "github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	evmtypes "github.com/sidiora-labs/paxeer-network/modules/evm/types"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types/ethtx"
	bridgetypes "github.com/sidiora-labs/paxeer-network/modules/layerxbridge/types"
	app "github.com/sidiora-labs/paxeer-network/node"
	"github.com/sidiora-labs/paxeer-network/occ_tests/utils"
	"github.com/sidiora-labs/paxeer-network/sdk/baseapp"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	paramproposal "github.com/sidiora-labs/paxeer-network/sdk/x/params/types/proposal"
	"github.com/stretchr/testify/require"

	gigaevmstate "github.com/sidiora-labs/paxeer-network/engine/deps/xevm/state"
)

const (
	// feeTokenGasPrice is 500 gwei, a fee per gas the parameters allow.
	feeTokenGasPrice = int64(500_000_000_000)
	// feeTokenGasLimit leaves 9000 of the gas unused by a plain transfer.
	feeTokenGasLimit = uint64(30_000)
	// feeTokenTransferGas is the gas a plain transfer uses.
	feeTokenTransferGas = uint64(21_000)
	// feeTokenPreCharged is 30000 gas at 500 gwei at 3.114 Sidiora per Paxeer coin.
	feeTokenPreCharged = int64(46_710)
	// feeTokenRefunded is the 9000 unused gas at 500 gwei at the same rate.
	feeTokenRefunded = int64(14_013)
	// feeTokenConverted is 21000 gas at 500 gwei at the same rate.
	feeTokenConverted = int64(32_697)
	// feeTokenInNetworkCoin is 21000 gas at 500 gwei in uhpx.
	feeTokenInNetworkCoin = int64(10_500)
	// feeTokenFunded is the Sidiora a payer holds, well above the fee.
	feeTokenFunded = int64(1_000_000)
	// feeTokenShort is less Sidiora than the converted fee.
	feeTokenShort = int64(1_000)
	// feeTokenNetworkFunding is the network coin a payer holds.
	feeTokenNetworkFunding = int64(1_000_000_000)
)

var feeTokenRecipient = common.HexToAddress("0x00000000000000000000000000000000000000de")

type feeTokenBlock struct {
	wrapper *app.TestWrapper
	ctx     sdk.Context
	payer   utils.TestAcct
	denom   string
}

// newFeeTokenBlock brings up an application on the giga block path with the
// fee-token switch on, Sidiora governed at the initial rate as of
// rateUpdateHeight, and a payer holding sidiora base units of Sidiora that prefers it.
func newFeeTokenBlock(t *testing.T, rateUpdateHeight int64, sidiora int64) *feeTokenBlock {
	app.EnableOCC = false
	t.Cleanup(func() { app.EnableOCC = true })
	accounts := utils.NewTestAccounts(1)
	wrapper := app.NewGigaTestWrapper(t, time.Now(), accounts[0].PublicKey, true, false, func(ba *baseapp.BaseApp) {
		ba.SetOccEnabled(false)
		ba.SetConcurrencyWorkers(1)
	})
	ctx := wrapper.Ctx
	denom := bridgetypes.SidioraDenom()

	params := wrapper.App.EvmKeeper.GetParams(ctx)
	params.FeeTokenEnabled = true
	params.MaxFeeTokenRateAge = evmtypes.DefaultMaxFeeTokenRateAge
	params.AllowedFeeDenoms = []evmtypes.AllowedFeeDenom{{
		Denom:            denom,
		Rate:             sdk.NewDec(evmtypes.InitialSidioraBaseUnitsPerPax),
		RateUpdateHeight: rateUpdateHeight,
	}}
	wrapper.App.EvmKeeper.SetParams(ctx, params)

	payer := utils.NewSigner()
	wrapper.App.EvmKeeper.SetAddressMapping(ctx, payer.AccountAddress, payer.EvmAddress)
	funding := sdk.NewCoins(
		sdk.NewCoin(denom, sdk.NewInt(sidiora)),
		sdk.NewCoin("uhpx", sdk.NewInt(feeTokenNetworkFunding)),
	)
	require.NoError(t, wrapper.App.BankKeeper.MintCoins(ctx, "mint", funding))
	require.NoError(t, wrapper.App.BankKeeper.SendCoinsFromModuleToAccount(ctx, "mint", payer.AccountAddress, funding))
	require.NoError(t, wrapper.App.EvmKeeper.SetAccountFeeDenom(ctx, payer.EvmAddress, denom))

	return &feeTokenBlock{wrapper: wrapper, ctx: ctx, payer: payer, denom: denom}
}

func (b *feeTokenBlock) setFeeTokenEnabled(enabled bool) {
	params := b.wrapper.App.EvmKeeper.GetParams(b.ctx)
	params.FeeTokenEnabled = enabled
	b.wrapper.App.EvmKeeper.SetParams(b.ctx, params)
}

// transfer signs a zero-value transfer from the payer at 500 gwei.
func (b *feeTokenBlock) transfer(t *testing.T, gas uint64) (*ethtypes.Transaction, []byte) {
	signed, err := ethtypes.SignTx(ethtypes.NewTx(&ethtypes.LegacyTx{
		Nonce:    b.wrapper.App.EvmKeeper.GetNonce(b.ctx, b.payer.EvmAddress),
		GasPrice: big.NewInt(feeTokenGasPrice),
		Gas:      gas,
		To:       &feeTokenRecipient,
		Value:    big.NewInt(0),
	}), ethtypes.LatestSignerForChainID(big.NewInt(config.DefaultChainID)), b.payer.EvmPrivateKey)
	require.NoError(t, err)
	txData, err := ethtx.NewTxDataFromTx(signed)
	require.NoError(t, err)
	msg, err := evmtypes.NewMsgEVMTransaction(txData)
	require.NoError(t, err)
	txConfig := app.MakeEncodingConfig().TxConfig
	builder := txConfig.NewTxBuilder()
	require.NoError(t, builder.SetMsgs(msg))
	builder.SetGasLimit(gas)
	bz, err := txConfig.TxEncoder()(builder.GetTx())
	require.NoError(t, err)
	return signed, bz
}

// run drives one block holding txs through the application's block path.
func (b *feeTokenBlock) run(t *testing.T, txs ...[]byte) []*abci.ExecTxResult {
	header := b.ctx.BlockHeader()
	_, results, _, err := b.wrapper.App.ProcessBlock(b.ctx, txs, &app.BlockProcessRequest{
		Height: header.Height,
		Time:   header.Time,
	}, abci.CommitInfo{}, false, nil)
	require.NoError(t, err)
	require.Len(t, results, len(txs))
	return results
}

func (b *feeTokenBlock) balance(addr sdk.AccAddress, denom string) sdk.Int {
	return b.wrapper.App.BankKeeper.GetBalance(b.ctx, addr, denom).Amount
}

func TestFeeTokenGigaBlockDebitsAndRefundsInFeeDenom(t *testing.T) {
	b := newFeeTokenBlock(t, 1, feeTokenFunded)
	uhpxBefore := b.balance(b.payer.AccountAddress, "uhpx")
	weiBefore := b.wrapper.App.BankKeeper.GetWeiBalance(b.ctx, b.payer.AccountAddress)

	tx, bz := b.transfer(t, feeTokenGasLimit)
	results := b.run(t, bz)
	require.Equal(t, uint32(0), results[0].Code, results[0].Log)
	require.Equal(t, int64(feeTokenTransferGas), results[0].GasUsed)

	// The pre-charge at the recorded rate less the refund at the same rate is
	// the converted fee of the gas the transfer used.
	require.Equal(t, feeTokenConverted, feeTokenPreCharged-feeTokenRefunded)
	require.Equal(t, sdk.NewInt(feeTokenFunded-feeTokenConverted), b.balance(b.payer.AccountAddress, b.denom))
	require.Equal(t, sdk.NewInt(feeTokenConverted), b.balance(gigaevmstate.GetCoinbaseAddress(0), b.denom))
	// The network coin pays none of the gas.
	require.Equal(t, uhpxBefore, b.balance(b.payer.AccountAddress, "uhpx"))
	require.Equal(t, weiBefore, b.wrapper.App.BankKeeper.GetWeiBalance(b.ctx, b.payer.AccountAddress))
	require.Equal(t, uint64(1), b.wrapper.App.EvmKeeper.GetNonce(b.ctx, b.payer.EvmAddress))

	charge, err := b.wrapper.App.EvmKeeper.GetAnteFeeTokenCharge(b.ctx, tx.Hash())
	require.NoError(t, err)
	require.NotNil(t, charge)
	require.Equal(t, b.payer.EvmAddress, charge.Payer)
	require.Equal(t, b.denom, charge.Denom)
	require.Equal(t, sdk.NewDec(evmtypes.InitialSidioraBaseUnitsPerPax), charge.Rate)
}

func TestFeeTokenGigaBlockRefusesShortBalance(t *testing.T) {
	b := newFeeTokenBlock(t, 1, feeTokenShort)
	uhpxBefore := b.balance(b.payer.AccountAddress, "uhpx")

	tx, bz := b.transfer(t, feeTokenTransferGas)
	results := b.run(t, bz)
	require.Equal(t, uint32(5), results[0].Code, results[0].Log)
	require.Contains(t, results[0].Log, "insufficient "+b.denom+" for gas")

	require.Equal(t, sdk.NewInt(feeTokenShort), b.balance(b.payer.AccountAddress, b.denom))
	require.Equal(t, uhpxBefore, b.balance(b.payer.AccountAddress, "uhpx"))
	require.Equal(t, uint64(1), b.wrapper.App.EvmKeeper.GetNonce(b.ctx, b.payer.EvmAddress))
	charge, err := b.wrapper.App.EvmKeeper.GetAnteFeeTokenCharge(b.ctx, tx.Hash())
	require.NoError(t, err)
	require.Nil(t, charge)
}

func TestFeeTokenGigaBlockRefusesUnusableRate(t *testing.T) {
	// A rate recorded above the height of the block is not usable.
	b := newFeeTokenBlock(t, 2, feeTokenFunded)
	uhpxBefore := b.balance(b.payer.AccountAddress, "uhpx")

	tx, bz := b.transfer(t, feeTokenTransferGas)
	results := b.run(t, bz)
	require.Equal(t, uint32(1), results[0].Code, results[0].Log)
	require.Contains(t, results[0].Log, "fee-token rate is invalid")

	require.Equal(t, sdk.NewInt(feeTokenFunded), b.balance(b.payer.AccountAddress, b.denom))
	require.Equal(t, uhpxBefore, b.balance(b.payer.AccountAddress, "uhpx"))
	require.Equal(t, uint64(1), b.wrapper.App.EvmKeeper.GetNonce(b.ctx, b.payer.EvmAddress))
	charge, err := b.wrapper.App.EvmKeeper.GetAnteFeeTokenCharge(b.ctx, tx.Hash())
	require.NoError(t, err)
	require.Nil(t, charge)
}

func TestFeeTokenGigaBlockSwitchOffChargesNetworkCoin(t *testing.T) {
	b := newFeeTokenBlock(t, 1, feeTokenFunded)
	b.setFeeTokenEnabled(false)
	uhpxBefore := b.balance(b.payer.AccountAddress, "uhpx")
	weiBefore := b.wrapper.App.BankKeeper.GetWeiBalance(b.ctx, b.payer.AccountAddress)

	tx, bz := b.transfer(t, feeTokenTransferGas)
	results := b.run(t, bz)
	require.Equal(t, uint32(0), results[0].Code, results[0].Log)

	require.Equal(t, uhpxBefore.Sub(sdk.NewInt(feeTokenInNetworkCoin)), b.balance(b.payer.AccountAddress, "uhpx"))
	require.Equal(t, weiBefore, b.wrapper.App.BankKeeper.GetWeiBalance(b.ctx, b.payer.AccountAddress))
	require.Equal(t, sdk.NewInt(feeTokenFunded), b.balance(b.payer.AccountAddress, b.denom))
	require.True(t, b.balance(gigaevmstate.GetCoinbaseAddress(0), b.denom).IsZero())
	charge, err := b.wrapper.App.EvmKeeper.GetAnteFeeTokenCharge(b.ctx, tx.Hash())
	require.NoError(t, err)
	require.Nil(t, charge)
}

func TestFeeTokenRateBoundAppRoutesParamChanges(t *testing.T) {
	b := newFeeTokenBlock(t, 1, feeTokenFunded)
	stored := b.wrapper.App.EvmKeeper.GetAllowedFeeDenoms(b.ctx)
	beyond := []evmtypes.AllowedFeeDenom{{Denom: b.denom, Rate: stored[0].Rate.Mul(sdk.NewDecWithPrec(11, 1)), RateUpdateHeight: 2}}
	value, err := b.wrapper.App.LegacyAmino().MarshalAsJSON(beyond)
	require.NoError(t, err)
	content := paramproposal.NewParameterChangeProposal("rate", "rate", []paramproposal.ParamChange{
		paramproposal.NewParamChange(evmtypes.ModuleName, string(evmtypes.KeyAllowedFeeDenoms), string(value)),
	}, false)
	route := b.wrapper.App.GovKeeper.Router().GetRoute(paramproposal.RouterKey)
	require.ErrorIs(t, route(b.ctx, content), evmkeeper.ErrFeeTokenRateSpread)
	require.Equal(t, stored, b.wrapper.App.EvmKeeper.GetAllowedFeeDenoms(b.ctx))
}
