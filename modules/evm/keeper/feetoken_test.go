package keeper_test

import (
	"github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	"math/big"
	"testing"

	"github.com/ethereum/go-ethereum/common"
	tmtypes "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/types"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

func TestAccountFeeDenomStoreRoundTrip(t *testing.T) {
	app := testkeeper.EVMTestApp
	ctx, _ := app.NewContext(false, tmtypes.Header{}).WithBlockHeight(11).CacheContext()
	k := &app.EvmKeeper
	account := common.HexToAddress("0x1234")
	other := common.HexToAddress("0x5678")
	require.Equal(t, k.GetBaseDenom(ctx), k.GetAccountFeeDenom(ctx, account))
	params := types.DefaultParams()
	params.FeeTokenEnabled = true
	params.AllowedFeeDenoms = []types.AllowedFeeDenom{{Denom: "usid", Rate: sdk.NewDec(types.InitialSidioraBaseUnitsPerPax), RateUpdateHeight: ctx.BlockHeight()}, {Denom: "uasset", Rate: sdk.NewDec(1_000_000), RateUpdateHeight: ctx.BlockHeight()}}
	k.SetParams(ctx, params)
	require.NoError(t, k.SetAccountFeeDenom(ctx, account, "usid"))
	require.Equal(t, []byte("usid"), ctx.KVStore(app.GetKey(types.StoreKey)).Get(types.AccountFeeDenomKey(account)))
	require.Equal(t, "usid", k.GetAccountFeeDenom(ctx, account))
	require.Equal(t, "uhpx", k.GetAccountFeeDenom(ctx, other))
	require.NoError(t, k.SetAccountFeeDenom(ctx, account, "uasset"))
	require.Equal(t, "uasset", k.GetAccountFeeDenom(ctx, account))
	require.ErrorContains(t, k.SetAccountFeeDenom(ctx, account, "unknown"), "not allowed")
	require.Equal(t, "uasset", k.GetAccountFeeDenom(ctx, account))
	k.Paramstore.Set(ctx, types.KeyFeeTokenEnabled, false)
	require.ErrorContains(t, k.SetAccountFeeDenom(ctx, account, "usid"), "switch is off")
	k.Paramstore.Set(ctx, types.KeyAllowedFeeDenoms, []types.AllowedFeeDenom{})
	require.Equal(t, "uasset", k.GetAccountFeeDenom(ctx, account))
	for _, addr := range []common.Address{account, other} {
		meter := sdk.NewGasMeter(0, 1, 1)
		require.NotEmpty(t, k.GetAccountFeeDenom(ctx.WithGasMeter(meter), addr))
		require.Zero(t, meter.GasConsumed())
	}
	k.ClearAccountFeeDenom(ctx, account)
	require.False(t, ctx.KVStore(app.GetKey(types.StoreKey)).Has(types.AccountFeeDenomKey(account)))
	require.Equal(t, "uhpx", k.GetAccountFeeDenom(ctx, account))
	k.ClearAccountFeeDenom(ctx, account)
	require.Equal(t, "uhpx", k.GetAccountFeeDenom(ctx, account))
}

func TestFeeTokenConversions(t *testing.T) {
	rate := sdk.NewDec(types.InitialSidioraBaseUnitsPerPax)
	wei := sdk.NewInt(1_000_000_000_000_000_000)
	sid, err := keeper.ConvertFeeToDenom(wei, rate, true)
	require.NoError(t, err)
	require.Equal(t, sdk.NewInt(3_114_000), sid)
	restored, err := keeper.ConvertFeeFromDenom(sid, rate, false)
	require.NoError(t, err)
	require.Equal(t, wei, restored)
	for _, convert := range []func(sdk.Int, sdk.Dec, bool) (sdk.Int, error){keeper.ConvertFeeToDenom, keeper.ConvertFeeFromDenom} {
		for _, invalid := range []sdk.Dec{sdk.Dec{}, sdk.ZeroDec(), sdk.NewDec(-1)} {
			got, err := convert(wei, invalid, true)
			require.ErrorIs(t, err, keeper.ErrFeeTokenRateInvalid)
			require.True(t, got.IsNil())
		}
		got, err := convert(sdk.NewInt(-1), rate, true)
		require.ErrorIs(t, err, keeper.ErrFeeTokenAmountInvalid)
		require.True(t, got.IsNil())
		got, err = convert(sdk.Int{}, rate, true)
		require.ErrorIs(t, err, keeper.ErrFeeTokenAmountInvalid)
		require.True(t, got.IsNil())
		got, err = convert(sdk.ZeroInt(), rate, true)
		require.NoError(t, err)
		require.True(t, got.IsZero())
	}
	floor, err := keeper.ConvertFeeToDenom(sdk.OneInt(), rate, false)
	require.NoError(t, err)
	require.True(t, floor.IsZero())
	for _, purpose := range []string{"charge", "refund"} {
		t.Run(purpose, func(t *testing.T) {
			ceiling, err := keeper.ConvertFeeToDenom(sdk.OneInt(), rate, true)
			require.NoError(t, err)
			require.Equal(t, sdk.OneInt(), ceiling)
		})
	}
	floor, err = keeper.ConvertFeeFromDenom(sdk.OneInt(), sdk.NewDec(3_000_000), false)
	require.NoError(t, err)
	require.Equal(t, sdk.NewInt(333_333_333_333), floor)
	ceiling, err := keeper.ConvertFeeFromDenom(sdk.OneInt(), sdk.NewDec(3_000_000), true)
	require.NoError(t, err)
	require.Equal(t, sdk.NewInt(333_333_333_334), ceiling)
	maximum := sdk.NewIntFromBigInt(new(big.Int).Sub(new(big.Int).Lsh(big.NewInt(1), 256), big.NewInt(1)))
	_, err = keeper.ConvertFeeToDenom(maximum, sdk.NewDec(2_000_000_000_000_000_000), true)
	require.ErrorIs(t, err, keeper.ErrFeeTokenOverflow)
	_, err = keeper.ConvertFeeFromDenom(maximum, sdk.OneDec(), true)
	require.ErrorIs(t, err, keeper.ErrFeeTokenOverflow)
	exact, err := keeper.ConvertFeeToDenom(maximum, sdk.NewDec(1_000_000_000_000_000_000), true)
	require.NoError(t, err)
	require.Equal(t, maximum, exact)
	tiny, err := keeper.ConvertFeeToDenom(sdk.OneInt(), sdk.NewDecWithPrec(1, 18), true)
	require.NoError(t, err)
	require.Equal(t, sdk.OneInt(), tiny)
}

func TestFeeTokenChargeReaderAndRecord(t *testing.T) {
	app := testkeeper.EVMTestApp
	k := &app.EvmKeeper
	ctx, _ := app.GetContextForDeliverTx(nil).WithBlockHeight(100).CacheContext()
	payer := common.HexToAddress("0x9876")
	params := types.DefaultParams()
	params.FeeTokenEnabled = true
	params.AllowedFeeDenoms = []types.AllowedFeeDenom{{Denom: "usid", Rate: sdk.NewDec(types.InitialSidioraBaseUnitsPerPax), RateUpdateHeight: 100}}
	k.SetParams(ctx, params)
	charge, err := k.GetFeeTokenCharge(ctx, payer)
	require.NoError(t, err)
	require.Nil(t, charge)
	ctx.KVStore(app.GetKey(types.StoreKey)).Set(types.AccountFeeDenomKey(payer), []byte("uhpx"))
	charge, err = k.GetFeeTokenCharge(ctx, payer)
	require.NoError(t, err)
	require.Nil(t, charge)
	require.NoError(t, k.SetAccountFeeDenom(ctx, payer, "usid"))
	charge, err = k.GetFeeTokenCharge(ctx, payer)
	require.NoError(t, err)
	require.Equal(t, "usid", charge.Denom)
	hash := common.HexToHash("0x1234")
	require.NoError(t, k.SetAnteFeeTokenCharge(ctx, hash, charge))
	recorded, err := k.GetAnteFeeTokenCharge(ctx, hash)
	require.NoError(t, err)
	require.Equal(t, charge, recorded)
	absent, err := k.GetAnteFeeTokenCharge(ctx, common.HexToHash("0x5678"))
	require.NoError(t, err)
	require.Nil(t, absent)
	_, err = k.GetFeeTokenCharge(ctx.WithBlockHeight(1101), payer)
	require.ErrorIs(t, err, keeper.ErrFeeTokenRateStale)
	_, err = k.GetFeeTokenCharge(ctx.WithBlockHeight(99), payer)
	require.ErrorIs(t, err, keeper.ErrFeeTokenRateInvalid)
	k.Paramstore.Set(ctx, types.KeyFeeTokenEnabled, false)
	disabled, err := k.GetFeeTokenCharge(ctx, payer)
	require.NoError(t, err)
	require.Nil(t, disabled)
	k.Paramstore.Set(ctx, types.KeyFeeTokenEnabled, true)
	k.Paramstore.Set(ctx, types.KeyAllowedFeeDenoms, []types.AllowedFeeDenom{})
	_, err = k.GetFeeTokenCharge(ctx, payer)
	require.ErrorIs(t, err, keeper.ErrFeeTokenDenomNotAllowed)
	require.ErrorIs(t, err, keeper.ErrFeeTokenRateUnavailable)
	require.Contains(t, err.Error(), "usid")
	recorded, err = k.GetAnteFeeTokenCharge(ctx, hash)
	require.NoError(t, err)
	require.Equal(t, charge, recorded)
	require.NoError(t, k.SetAnteFeeTokenCharge(ctx, hash, nil))
	absent, err = k.GetAnteFeeTokenCharge(ctx, hash)
	require.NoError(t, err)
	require.Nil(t, absent)
}
