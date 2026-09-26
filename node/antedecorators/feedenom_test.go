package antedecorators_test

import (
	"math"
	"math/big"
	"testing"

	evmkeeper "github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	evmtypes "github.com/sidiora-labs/paxeer-network/modules/evm/types"
	"github.com/sidiora-labs/paxeer-network/node/antedecorators"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/sdk/x/auth/ante"
	banktypes "github.com/sidiora-labs/paxeer-network/sdk/x/bank/types"
	paramskeeper "github.com/sidiora-labs/paxeer-network/sdk/x/params/keeper"
	paramtypes "github.com/sidiora-labs/paxeer-network/sdk/x/params/types"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

func feeDenomContext(t *testing.T) sdk.Context {
	t.Helper()
	app := testkeeper.EVMTestApp
	ctx, _ := app.GetContextForDeliverTx(nil).WithBlockHeight(100).WithIsCheckTx(true).WithTxIndex(0).CacheContext()
	params := app.EvmKeeper.GetParams(ctx)
	params.FeeTokenEnabled = true
	params.MaxFeeTokenRateAge = 10
	params.AllowedFeeDenoms = []evmtypes.AllowedFeeDenom{{Denom: "usid", Rate: sdk.NewDec(3_000_000), RateUpdateHeight: 100}}
	app.EvmKeeper.SetParams(ctx, params)
	app.ParamsKeeper.SetFeesParams(ctx, paramtypes.FeesParams{GlobalMinimumGasPrices: sdk.DecCoins{}, AllowedFeeDenoms: []string{"uother"}})
	return ctx.WithMinGasPrices(sdk.DecCoins{})
}

func feeDenomTx(t *testing.T, fees sdk.Coins, gas uint64) sdk.FeeTx {
	t.Helper()
	builder := testkeeper.EVMTestApp.GetTxConfig().NewTxBuilder()
	payer := sdk.AccAddress([]byte("fee-denom-payer_____"))
	require.NoError(t, builder.SetMsgs(&banktypes.MsgSend{FromAddress: payer.String(), ToAddress: payer.String(), Amount: sdk.NewCoins(sdk.NewInt64Coin("uhpx", 1))}))
	builder.SetFeeAmount(fees)
	builder.SetGasLimit(gas)
	return builder.GetTx()
}

func TestFeeDenomDeductSidiora(t *testing.T) {
	ctx := feeDenomContext(t)
	app := testkeeper.EVMTestApp
	fee := sdk.NewCoins(sdk.NewInt64Coin("usid", 30))
	tx := feeDenomTx(t, fee, 100)
	payer := tx.FeePayer()
	app.AccountKeeper.SetAccount(ctx, app.AccountKeeper.NewAccountWithAddress(ctx, payer))
	require.NoError(t, app.BankKeeper.AddCoins(ctx, payer, sdk.NewCoins(sdk.NewInt64Coin("usid", 100)), true))
	deduct := ante.NewDeductFeeDecorator(app.AccountKeeper, app.BankKeeper, app.FeeGrantKeeper, app.ParamsKeeper, antedecorators.NewFeeDenomTxFeeChecker(&app.EvmKeeper, nil))
	result, err := sdk.ChainAnteDecorators(deduct)(ctx, tx, false)
	require.NoError(t, err)
	require.Equal(t, sdk.NewInt(70), app.BankKeeper.GetBalance(result, payer, "usid").Amount)
	require.True(t, app.BankKeeper.GetBalance(result, payer, "uhpx").IsZero())
	require.Equal(t, ante.GetTxPriority(sdk.NewCoins(sdk.NewInt64Coin("uhpx", 10_000_000_000_000)), 100), result.Priority())
}

func TestFeeDenomMinimumAndPriority(t *testing.T) {
	app := testkeeper.EVMTestApp
	checker := antedecorators.NewFeeDenomTxFeeChecker(&app.EvmKeeper, nil)
	for _, global := range []bool{false, true} {
		for _, amount := range []int64{29, 30, 31} {
			ctx := feeDenomContext(t)
			minimum := sdk.NewDecCoins(sdk.NewInt64DecCoin("uhpx", 100_000_000_000))
			if global {
				params := app.ParamsKeeper.GetFeesParams(ctx)
				params.GlobalMinimumGasPrices = minimum
				app.ParamsKeeper.SetFeesParams(ctx, params)
			} else {
				ctx = ctx.WithMinGasPrices(minimum)
			}
			sid := sdk.NewCoins(sdk.NewInt64Coin("usid", amount))
			paxAmount, err := app.EvmKeeper.ConvertFeeFromDenom(sdk.NewInt(amount), sdk.NewDec(3_000_000), false)
			require.NoError(t, err)
			pax := sdk.NewCoins(sdk.NewCoin("uhpx", paxAmount))
			fee, priority, err := checker(ctx, feeDenomTx(t, sid, 100), false, app.ParamsKeeper)
			_, paxPriority, paxErr := checker(ctx, feeDenomTx(t, pax, 100), false, app.ParamsKeeper)
			if amount < 30 {
				require.Error(t, err)
				require.Error(t, paxErr)
			} else {
				require.NoError(t, err)
				require.NoError(t, paxErr)
				require.Equal(t, sid, fee)
				require.Equal(t, paxPriority, priority)
			}
			for _, mode := range []struct{ check, simulate bool }{{false, false}, {true, true}} {
				_, _, err := checker(ctx.WithIsCheckTx(mode.check), feeDenomTx(t, sid, 100), mode.simulate, app.ParamsKeeper)
				require.NoError(t, err)
			}
		}
	}
}

func TestFeeDenomRefusals(t *testing.T) {
	app := testkeeper.EVMTestApp
	for _, tc := range []struct {
		name   string
		rate   sdk.Dec
		height int64
		amount sdk.Int
		denom  string
		want   error
	}{
		{"disallowed", sdk.OneDec(), 100, sdk.OneInt(), "ubad", evmkeeper.ErrFeeTokenDenomNotAllowed},
		{"missing", sdk.OneDec(), 100, sdk.OneInt(), "umissing", evmkeeper.ErrFeeTokenRateUnavailable},
		{"zero rate", sdk.ZeroDec(), 100, sdk.OneInt(), "usid", evmkeeper.ErrFeeTokenRateInvalid},
		{"negative rate", sdk.NewDec(-1), 100, sdk.OneInt(), "usid", evmkeeper.ErrFeeTokenRateInvalid},
		{"future rate", sdk.OneDec(), 101, sdk.OneInt(), "usid", evmkeeper.ErrFeeTokenRateInvalid},
		{"stale rate", sdk.OneDec(), 89, sdk.OneInt(), "usid", evmkeeper.ErrFeeTokenRateStale},
		{"overflow", sdk.OneDec(), 100, sdk.NewIntFromBigInt(new(big.Int).Lsh(big.NewInt(1), 255)), "usid", evmkeeper.ErrFeeTokenOverflow},
	} {
		t.Run(tc.name, func(t *testing.T) {
			ctx := feeDenomContext(t)
			app.EvmKeeper.Paramstore.Set(ctx, evmtypes.KeyAllowedFeeDenoms, []evmtypes.AllowedFeeDenom{{Denom: "usid", Rate: tc.rate, RateUpdateHeight: tc.height}})
			checker := antedecorators.NewFeeDenomTxFeeChecker(&app.EvmKeeper, nil)
			fee, priority, err := checker(ctx, feeDenomTx(t, sdk.NewCoins(sdk.NewCoin(tc.denom, tc.amount)), 100), false, app.ParamsKeeper)
			require.ErrorIs(t, err, tc.want)
			require.Contains(t, err.Error(), tc.denom)
			require.Nil(t, fee)
			require.Zero(t, priority)
		})
	}
}

func TestFeeDenomDelegation(t *testing.T) {
	app := testkeeper.EVMTestApp
	for _, enabled := range []bool{false, true} {
		for _, denom := range []string{"uhpx", "uother", "usid"} {
			ctx := feeDenomContext(t)
			app.EvmKeeper.Paramstore.Set(ctx, evmtypes.KeyFeeTokenEnabled, enabled)
			tx := feeDenomTx(t, sdk.NewCoins(sdk.NewInt64Coin(denom, 30)), 100)
			calls := 0
			inner := func(ctx sdk.Context, received sdk.Tx, simulate bool, pk paramskeeper.Keeper) (sdk.Coins, int64, error) {
				calls++
				if !enabled || denom != "usid" {
					require.Same(t, tx, received)
				} else {
					require.Equal(t, sdk.NewCoins(sdk.NewInt64Coin("uhpx", 10_000_000_000_000)), received.(sdk.FeeTx).GetFee())
				}
				return ante.CheckTxFeeWithValidatorMinGasPrices(ctx, received, simulate, pk)
			}
			fee, priority, err := antedecorators.NewFeeDenomTxFeeChecker(&app.EvmKeeper, inner)(ctx, tx, false, app.ParamsKeeper)
			require.NoError(t, err)
			require.Equal(t, 1, calls)
			if !enabled || denom != "usid" {
				wantFee, wantPriority, wantErr := ante.CheckTxFeeWithValidatorMinGasPrices(ctx, tx, false, app.ParamsKeeper)
				require.Equal(t, wantErr, err)
				require.Equal(t, wantFee, fee)
				require.Equal(t, wantPriority, priority)
			}
		}
	}
}

func TestFeeDenomMixedAndRounding(t *testing.T) {
	app := testkeeper.EVMTestApp
	ctx := feeDenomContext(t).WithMinGasPrices(sdk.NewDecCoins(sdk.NewInt64DecCoin("uhpx", 1_000_000_000_000)))
	checker := antedecorators.NewFeeDenomTxFeeChecker(&app.EvmKeeper, nil)
	for _, pax := range []int64{666_666_666_666, 666_666_666_667} {
		fee := sdk.NewCoins(sdk.NewInt64Coin("usid", 1), sdk.NewInt64Coin("uhpx", pax))
		got, _, err := checker(ctx, feeDenomTx(t, fee, 1), false, app.ParamsKeeper)
		if pax == 666_666_666_666 {
			require.Error(t, err)
		} else {
			require.NoError(t, err)
			require.Equal(t, fee, got)
		}
	}
	fee := sdk.NewCoins(sdk.NewInt64Coin("usid", 1), sdk.NewInt64Coin("ubad", 1))
	_, _, err := checker(ctx, feeDenomTx(t, fee, 1), false, app.ParamsKeeper)
	require.ErrorContains(t, err, "ubad")
	_, _, err = checker(ctx, feeDenomTx(t, sdk.NewCoins(sdk.NewInt64Coin("usid", 1)), math.MaxUint64), false, app.ParamsKeeper)
	require.ErrorContains(t, err, "exceeds max int64")
}
