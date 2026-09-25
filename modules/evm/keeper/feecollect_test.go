package keeper_test

import (
	"bytes"
	"compress/gzip"
	"io"
	"testing"
	"time"

	"github.com/gogo/protobuf/proto"
	"github.com/gogo/protobuf/protoc-gen-gogo/descriptor"
	abci "github.com/sidiora-labs/paxeer-network/consensus/abci/types"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	app "github.com/sidiora-labs/paxeer-network/node"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	authtypes "github.com/sidiora-labs/paxeer-network/sdk/x/auth/types"
	"github.com/sidiora-labs/paxeer-network/sdk/x/distribution"
	distrtypes "github.com/sidiora-labs/paxeer-network/sdk/x/distribution/types"
	"github.com/stretchr/testify/require"
)

func TestSidioraFeeCollectionBlock(t *testing.T) {
	for _, distribute := range []bool{false, true} {
		name := "hold"
		if distribute {
			name = "distribute"
		}
		t.Run(name, func(t *testing.T) {
			testApp := app.Setup(t, false, false, false)
			ctx := testApp.GetContextForDeliverTx(nil).WithBlockHeight(1).WithBlockTime(time.Now())
			k := &testApp.EvmKeeper
			params := types.DefaultParams()
			params.AllowedFeeDenoms = []types.AllowedFeeDenom{
				{Denom: "usid", Rate: sdk.NewDec(types.InitialSidioraBaseUnitsPerPax), RateUpdateHeight: ctx.BlockHeight()},
				{Denom: "uasset", Rate: sdk.NewDec(1_000_000), RateUpdateHeight: ctx.BlockHeight()},
			}
			params.FeeTokenEnabled = true
			params.FeeTokenDistribution = distribute
			k.SetParams(ctx, params)

			holding := testApp.AccountKeeper.GetModuleAccount(ctx, types.FeeTokenHoldingAccount)
			require.NotNil(t, holding)
			require.Empty(t, holding.GetPermissions())
			require.False(t, holding.HasPermission(authtypes.Minter))
			require.False(t, holding.HasPermission(authtypes.Burner))
			require.True(t, testApp.BankKeeper.BlockedAddr(holding.GetAddress()))
			require.Panics(t, func() {
				_ = testApp.BankKeeper.MintCoins(ctx, types.FeeTokenHoldingAccount, sdk.NewCoins(sdk.NewInt64Coin("usid", 1)))
			})

			fees := sdk.NewCoins(sdk.NewInt64Coin("usid", 700), sdk.NewInt64Coin("uasset", 300), sdk.NewInt64Coin("uhpx", 1100), sdk.NewInt64Coin("uother", 90))
			require.NoError(t, testApp.BankKeeper.MintCoins(ctx, types.ModuleName, fees))
			require.NoError(t, testApp.BankKeeper.SendCoinsFromModuleToModule(ctx, types.ModuleName, authtypes.FeeCollectorName, fees))
			collector := testApp.AccountKeeper.GetModuleAddress(authtypes.FeeCollectorName)
			_, _, _, err := testApp.ProcessBlock(ctx, nil, &app.BlockProcessRequest{Height: 1}, abci.CommitInfo{}, false, nil)
			require.NoError(t, err)

			held := sdk.NewCoins(sdk.NewInt64Coin("usid", 700), sdk.NewInt64Coin("uasset", 300))
			remaining := fees.Sub(held)
			if distribute {
				held = sdk.NewCoins()
				remaining = fees
			}
			require.Equal(t, held, testApp.BankKeeper.GetAllBalances(ctx, holding.GetAddress()))
			require.Equal(t, remaining, testApp.BankKeeper.GetAllBalances(ctx, collector))
			require.Equal(t, sdk.NewInt(1100), testApp.BankKeeper.GetBalance(ctx, collector, "uhpx").Amount)
			require.NoError(t, k.RouteCollectedFeeTokens(ctx))
			require.Equal(t, held, testApp.BankKeeper.GetAllBalances(ctx, holding.GetAddress()))

			distribution.BeginBlocker(ctx.WithBlockHeight(2), nil, testApp.DistrKeeper)
			distributionAddress := testApp.AccountKeeper.GetModuleAddress(distrtypes.ModuleName)
			require.Equal(t, remaining, testApp.BankKeeper.GetAllBalances(ctx, distributionAddress))
			require.True(t, testApp.BankKeeper.GetAllBalances(ctx, collector).Empty())
			require.Equal(t, held, testApp.BankKeeper.GetAllBalances(ctx, holding.GetAddress()))
			require.Equal(t, sdk.NewDecCoinsFromCoins(remaining...), testApp.DistrKeeper.GetFeePool(ctx).CommunityPool)

			k.Paramstore.Set(ctx, types.KeyFeeTokenDistribution, !distribute)
			require.NoError(t, k.RouteCollectedFeeTokens(ctx))
			require.Equal(t, held, testApp.BankKeeper.GetAllBalances(ctx, holding.GetAddress()))
		})
	}
}

func TestSidioraFeeCollectionParams(t *testing.T) {
	k, ctx := feeTokenParamsKeeper(t)
	require.False(t, types.DefaultParams().FeeTokenDistribution)
	require.False(t, k.GetFeeTokenDistribution(ctx))
	require.False(t, k.Paramstore.Has(ctx, types.KeyFeeTokenDistribution))
	require.NoError(t, k.RouteCollectedFeeTokens(ctx))

	for _, distribute := range []bool{true, false} {
		params := types.DefaultParams()
		params.FeeTokenDistribution = distribute
		params.FeeTokenEnabled = true
		params.AllowedFeeDenoms = []types.AllowedFeeDenom{{Denom: "usid", Rate: sdk.NewDec(types.InitialSidioraBaseUnitsPerPax), RateUpdateHeight: ctx.BlockHeight()}}
		require.NoError(t, params.Validate())
		k.SetParams(ctx, params)
		require.Equal(t, params, k.GetParams(ctx))
		require.Equal(t, distribute, k.GetFeeTokenDistribution(ctx))
		encoded, err := params.Marshal()
		require.NoError(t, err)
		require.Len(t, encoded, params.Size())
		var decoded types.Params
		require.NoError(t, decoded.Unmarshal(encoded))
		require.Equal(t, params, decoded)
		require.Equal(t, distribute, decoded.GetFeeTokenDistribution())
	}

	params := types.DefaultParams()
	found := false
	for _, pair := range params.ParamSetPairs() {
		if bytes.Equal(pair.Key, types.KeyFeeTokenDistribution) {
			found = true
			require.NoError(t, pair.ValidatorFn(true))
			require.NoError(t, pair.ValidatorFn(false))
			for _, invalid := range []interface{}{nil, "true", 1} {
				require.ErrorContains(t, pair.ValidatorFn(invalid), "fee_token_distribution")
			}
		}
	}
	require.True(t, found)

	compressed, _ := params.Descriptor()
	reader, err := gzip.NewReader(bytes.NewReader(compressed))
	require.NoError(t, err)
	data, err := io.ReadAll(reader)
	require.NoError(t, err)
	require.NoError(t, reader.Close())
	var file descriptor.FileDescriptorProto
	require.NoError(t, proto.Unmarshal(data, &file))
	fields := map[string]int32{}
	for _, field := range file.MessageType[0].Field {
		fields[field.GetName()] = field.GetNumber()
	}
	require.Equal(t, int32(16), fields["allowed_fee_denoms"])
	require.Equal(t, int32(17), fields["max_fee_token_spread"])
	require.Equal(t, int32(18), fields["fee_token_enabled"])
	require.Equal(t, int32(19), fields["fee_token_distribution"])
}
