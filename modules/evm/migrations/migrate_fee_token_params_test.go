package migrations_test

import (
	"testing"

	tmtypes "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/types"
	"github.com/sidiora-labs/paxeer-network/modules/evm/migrations"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	paramstypes "github.com/sidiora-labs/paxeer-network/sdk/x/params/types"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

// feeTokenParamKeys are the five parameters the fee token adds to x/evm.
var feeTokenParamKeys = [][]byte{
	types.KeyFeeTokenEnabled,
	types.KeyAllowedFeeDenoms,
	types.KeyMaxFeeTokenSpread,
	types.KeyMaxFeeTokenRateAge,
	types.KeyFeeTokenDistribution,
}

// dropParams removes the named pairs from the x/evm parameter subspace, leaving
// the store in the shape a chain that predates those parameters carries.
func dropParams(ctx sdk.Context, keys ...[]byte) {
	store := ctx.KVStore(testkeeper.EVMTestApp.GetKey(paramstypes.StoreKey))
	for _, key := range keys {
		store.Delete(append([]byte(types.ModuleName+"/"), key...))
	}
}

func TestMigrateFeeTokenParamsSetsTheDefaultsAndKeepsStoredParams(t *testing.T) {
	k := testkeeper.EVMTestApp.EvmKeeper
	ctx := testkeeper.EVMTestApp.NewContext(false, tmtypes.Header{})

	original := k.GetParams(ctx)
	t.Cleanup(func() { k.SetParams(ctx, original) })

	stored := original
	stored.MinimumFeePerGas = sdk.NewDec(777)
	stored.TargetGasUsedPerBlock = 1234567
	k.SetParams(ctx, stored)

	dropParams(ctx, feeTokenParamKeys...)
	for _, key := range feeTokenParamKeys {
		require.False(t, k.Paramstore.Has(ctx, key), string(key))
	}

	require.NoError(t, migrations.MigrateFeeTokenParams(ctx, &k))

	for _, key := range feeTokenParamKeys {
		require.True(t, k.Paramstore.Has(ctx, key), string(key))
	}
	migrated := k.GetParams(ctx)
	require.Equal(t, types.DefaultFeeTokenEnabled, migrated.FeeTokenEnabled)
	require.Empty(t, migrated.AllowedFeeDenoms)
	require.Equal(t, types.DefaultMaxFeeTokenSpread, migrated.MaxFeeTokenSpread)
	require.Equal(t, types.DefaultMaxFeeTokenRateAge, migrated.MaxFeeTokenRateAge)
	require.Equal(t, types.DefaultFeeTokenDistribution, migrated.FeeTokenDistribution)

	require.Equal(t, sdk.NewDec(777), migrated.MinimumFeePerGas)
	require.Equal(t, uint64(1234567), migrated.TargetGasUsedPerBlock)
	require.Equal(t, stored.PriorityNormalizer, migrated.PriorityNormalizer)
	require.Equal(t, stored.BaseFeePerGas, migrated.BaseFeePerGas)
	require.Equal(t, stored.MaximumFeePerGas, migrated.MaximumFeePerGas)
	require.Equal(t, stored.MaxDynamicBaseFeeUpwardAdjustment, migrated.MaxDynamicBaseFeeUpwardAdjustment)
	require.Equal(t, stored.MaxDynamicBaseFeeDownwardAdjustment, migrated.MaxDynamicBaseFeeDownwardAdjustment)
	require.Equal(t, stored.DeliverTxHookWasmGasLimit, migrated.DeliverTxHookWasmGasLimit)
	require.Equal(t, stored.PaxSstoreSetGasEip2200, migrated.PaxSstoreSetGasEip2200)
	require.Equal(t, stored.RegisterPointerDisabled, migrated.RegisterPointerDisabled)
	require.Equal(t, stored.WhitelistedCwCodeHashesForDelegateCall, migrated.WhitelistedCwCodeHashesForDelegateCall)
}

func TestMigrateFeeTokenParamsKeepsAGovernedFeeTokenParam(t *testing.T) {
	k := testkeeper.EVMTestApp.EvmKeeper
	ctx := testkeeper.EVMTestApp.NewContext(false, tmtypes.Header{})

	original := k.GetParams(ctx)
	t.Cleanup(func() { k.SetParams(ctx, original) })

	governedSpread := sdk.NewDecWithPrec(11, 2)
	require.NotEqual(t, types.DefaultMaxFeeTokenSpread, governedSpread)
	governedAge := types.DefaultMaxFeeTokenRateAge + 250

	stored := original
	stored.MaxFeeTokenSpread = governedSpread
	stored.MaxFeeTokenRateAge = governedAge
	k.SetParams(ctx, stored)

	dropParams(ctx, types.KeyFeeTokenEnabled, types.KeyAllowedFeeDenoms, types.KeyFeeTokenDistribution)

	require.NoError(t, migrations.MigrateFeeTokenParams(ctx, &k))

	migrated := k.GetParams(ctx)
	require.Equal(t, governedSpread, migrated.MaxFeeTokenSpread)
	require.Equal(t, governedAge, migrated.MaxFeeTokenRateAge)
	require.Equal(t, types.DefaultFeeTokenEnabled, migrated.FeeTokenEnabled)
	require.Equal(t, types.DefaultFeeTokenDistribution, migrated.FeeTokenDistribution)
	require.Empty(t, migrated.AllowedFeeDenoms)
}

func TestMigrateFeeTokenParamsIsIdempotent(t *testing.T) {
	k := testkeeper.EVMTestApp.EvmKeeper
	ctx := testkeeper.EVMTestApp.NewContext(false, tmtypes.Header{})

	original := k.GetParams(ctx)
	t.Cleanup(func() { k.SetParams(ctx, original) })

	dropParams(ctx, feeTokenParamKeys...)
	require.NoError(t, migrations.MigrateFeeTokenParams(ctx, &k))

	first := make(map[string][]byte, len(feeTokenParamKeys))
	for _, key := range feeTokenParamKeys {
		raw := k.Paramstore.GetRaw(ctx, key)
		require.NotEmpty(t, raw, string(key))
		first[string(key)] = raw
	}
	before := k.GetParams(ctx)

	require.NoError(t, migrations.MigrateFeeTokenParams(ctx, &k))

	for _, key := range feeTokenParamKeys {
		require.Equal(t, first[string(key)], k.Paramstore.GetRaw(ctx, key), string(key))
	}
	require.Equal(t, before, k.GetParams(ctx))
}
