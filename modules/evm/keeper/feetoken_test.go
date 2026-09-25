package keeper_test

import (
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
