package app_test

import (
	"math/big"
	"testing"
	"time"

	"github.com/ethereum/go-ethereum/common"
	abci "github.com/sidiora-labs/paxeer-network/consensus/abci/types"
	tmproto "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/types"
	evmtypes "github.com/sidiora-labs/paxeer-network/modules/evm/types"
	launchpadtypes "github.com/sidiora-labs/paxeer-network/modules/launchpad/types"
	layerxanchortypes "github.com/sidiora-labs/paxeer-network/modules/layerxanchor/types"
	layerxbridgetypes "github.com/sidiora-labs/paxeer-network/modules/layerxbridge/types"
	layerxcustodytypes "github.com/sidiora-labs/paxeer-network/modules/layerxcustody/types"
	layerxexchangetypes "github.com/sidiora-labs/paxeer-network/modules/layerxexchange/types"
	"github.com/sidiora-labs/paxeer-network/node"
	"github.com/sidiora-labs/paxeer-network/precompiles"
	"github.com/sidiora-labs/paxeer-network/precompiles/launchpad"
	"github.com/sidiora-labs/paxeer-network/precompiles/layerxbridge"
	"github.com/sidiora-labs/paxeer-network/precompiles/layerxexchange"
	"github.com/sidiora-labs/paxeer-network/sdk/crypto/keys/secp256k1"
	"github.com/sidiora-labs/paxeer-network/sdk/store/rootmulti"
	storetypes "github.com/sidiora-labs/paxeer-network/sdk/store/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/sdk/x/upgrade/types"
	"github.com/sidiora-labs/paxeer-network/storage/common/keys"
	"github.com/stretchr/testify/require"
	dbm "github.com/tendermint/tm-db"
)

func TestUpgradesListIsSorted(t *testing.T) {
	tm := time.Now().UTC()
	valPub := secp256k1.GenPrivKey().PubKey()
	testWrapper := app.NewTestWrapper(t, tm, valPub, false)
	testWrapper.App.RegisterUpgradeHandlers()
}

// Test community tax param is set to 0 as part of upgrade 1.2.3beta
func TestDistributionCommunityTaxParamMigration(t *testing.T) {
	tm := time.Now().UTC()
	valPub := secp256k1.GenPrivKey().PubKey()
	testWrapper := app.NewTestWrapper(t, tm, valPub, false)
	testWrapper.App.RegisterUpgradeHandlers()
	params := testWrapper.App.DistrKeeper.GetParams(testWrapper.Ctx)
	testWrapper.Require().Equal(params.CommunityTax, sdk.NewDec(0))
}

func TestSkipOptimisticProcessingOnUpgrade(t *testing.T) {
	t.Parallel()

	t.Run("Test optimistic processing is skipped on upgrade", func(t *testing.T) {
		tm := time.Now().UTC()
		valPub := secp256k1.GenPrivKey().PubKey()
		testWrapper := app.NewTestWrapper(t, tm, valPub, false)

		// No optimistic processing with upgrade scheduled
		testCtx := testWrapper.App.BaseApp.NewContext(false, tmproto.Header{Height: 3, ChainID: "pax-test", Time: tm})

		testWrapper.App.UpgradeKeeper.ScheduleUpgrade(testWrapper.Ctx, types.Plan{
			Name:   "test-plan",
			Height: testCtx.BlockHeight(),
		})
		plan, found := testWrapper.App.UpgradeKeeper.GetUpgradePlan(testCtx)
		require.True(t, found)
		require.True(t, plan.ShouldExecute(testCtx))

		res, _ := testWrapper.App.ProcessProposalHandler(testCtx, &abci.RequestProcessProposal{
			Header: &tmproto.Header{Height: 1, ChainID: "pax-test"},
		})
		require.Equal(t, res.Status, abci.ResponseProcessProposal_ACCEPT)
		require.True(t, testWrapper.App.GetOptimisticProcessingInfo().Aborted)
	})

	t.Run("Test optimistic processing if no upgrade", func(t *testing.T) {
		tm := time.Now().UTC()
		valPub := secp256k1.GenPrivKey().PubKey()
		testWrapper := app.NewTestWrapper(t, tm, valPub, false)
		testCtx := testWrapper.App.BaseApp.NewContext(false, tmproto.Header{Height: 3, ChainID: "pax-test", Time: tm})

		testWrapper.App.UpgradeKeeper.ScheduleUpgrade(testWrapper.Ctx, types.Plan{
			Name:   "test-plan",
			Height: testCtx.BlockHeight() + 1,
		})
		plan, found := testWrapper.App.UpgradeKeeper.GetUpgradePlan(testCtx)
		require.True(t, found)
		require.False(t, plan.ShouldExecute(testCtx))

		go func() {
			testWrapper.App.ProcessProposalHandler(testCtx, &abci.RequestProcessProposal{Header: &tmproto.Header{Height: 1, ChainID: "pax-test"}})
		}()

		require.Eventually(t, func() bool {
			opi := testWrapper.App.GetOptimisticProcessingInfo()
			if opi.Completion == nil {
				return false
			}
			<-opi.Completion
			return true
		}, 5*time.Second, time.Millisecond*100)

		// require.Equal(t, res.Status, abci.ResponseProcessProposal_ACCEPT)
		require.False(t, testWrapper.App.GetOptimisticProcessingInfo().Aborted)
	})
}

// v66Modules are the modules, and so the stores, a v6.4.0 chain gains at the v6.6
// Paxeer X fork.
var v66Modules = []string{
	layerxcustodytypes.StoreKey, layerxanchortypes.StoreKey,
	layerxexchangetypes.StoreKey, layerxbridgetypes.StoreKey, launchpadtypes.StoreKey,
}

func TestV66StoreUpgradesArePickedByPlanName(t *testing.T) {
	forkStores := []string{layerxexchangetypes.StoreKey, layerxbridgetypes.StoreKey, launchpadtypes.StoreKey}

	v66, ok := app.LayerXStoreUpgrades("v6.6")
	require.True(t, ok)
	require.Equal(t, app.V66StoreUpgrades(), v66)
	require.ElementsMatch(t, v66Modules, v66.Added)
	for _, name := range forkStores {
		require.True(t, v66.IsAdded(name), name)
	}

	v65, ok := app.LayerXStoreUpgrades("v6.5")
	require.True(t, ok)
	require.Equal(t, app.V65StoreUpgrades(), v65)
	require.Equal(t, []string{layerxcustodytypes.StoreKey}, v65.Added)
	for _, name := range forkStores {
		require.False(t, v65.IsAdded(name), name)
	}

	_, ok = app.LayerXStoreUpgrades("v6.4.0")
	require.False(t, ok)
}

func TestV640ToV66StoreUpgradeMountsTheNewStores(t *testing.T) {
	upgrades := app.V66StoreUpgrades()
	require.ElementsMatch(t, v66Modules, upgrades.Added)
	added := map[string]bool{}
	for _, name := range upgrades.Added {
		added[name] = true
	}

	db := dbm.NewMemDB()
	v640 := rootmulti.NewStore(db)
	v640.SetPruning(storetypes.PruneNothing)
	var bank *sdk.KVStoreKey
	for _, name := range keys.MemIAVLStoreKeys {
		if added[name] {
			continue
		}
		key := sdk.NewKVStoreKey(name)
		if name == keys.BankStoreKey {
			bank = key
		}
		v640.MountStoreWithDB(key, storetypes.StoreTypeIAVL, nil)
	}
	require.NoError(t, v640.LoadLatestVersion())
	v640.GetKVStore(bank).Set([]byte("balance"), []byte("kept"))
	require.Equal(t, int64(1), v640.Commit(true).Version)

	v66 := rootmulti.NewStore(db)
	v66.SetPruning(storetypes.PruneNothing)
	mounted := map[string]*sdk.KVStoreKey{}
	for _, name := range keys.MemIAVLStoreKeys {
		key := sdk.NewKVStoreKey(name)
		mounted[name] = key
		v66.MountStoreWithDB(key, storetypes.StoreTypeIAVL, nil)
	}
	require.NoError(t, v66.LoadLatestVersionAndUpgrade(&upgrades))
	require.Equal(t, []byte("kept"), v66.GetKVStore(mounted[keys.BankStoreKey]).Get([]byte("balance")))
	for _, name := range v66Modules {
		store := v66.GetKVStore(mounted[name])
		require.NotNil(t, store, name)
		iter := store.Iterator(nil, nil)
		require.False(t, iter.Valid(), name)
		require.NoError(t, iter.Close())
	}
	require.Equal(t, int64(2), v66.Commit(true).Version)
}

func TestV640ToV66UpgradeServesTheExchangeBridgeAndLaunchpadPrecompiles(t *testing.T) {
	tm := time.Now().UTC()
	valPub := secp256k1.GenPrivKey().PubKey()
	testWrapper := app.NewTestWrapper(t, tm, valPub, true)
	a, ctx := testWrapper.App, testWrapper.Ctx

	// Rewind the new modules to v6.4.0: no state and no consensus version.
	versions := ctx.KVStore(a.GetKey(types.StoreKey))
	for _, name := range v66Modules {
		store := ctx.KVStore(a.GetKey(name))
		iter := store.Iterator(nil, nil)
		var stale [][]byte
		for ; iter.Valid(); iter.Next() {
			stale = append(stale, iter.Key())
		}
		require.NoError(t, iter.Close())
		for _, key := range stale {
			store.Delete(key)
		}
		versions.Delete(append([]byte{types.VersionMapByte}, name...))
	}
	before := a.UpgradeKeeper.GetModuleVersionMap(ctx)
	for _, name := range v66Modules {
		require.NotContains(t, before, name)
	}

	require.True(t, a.UpgradeKeeper.HasHandler("v6.5"))
	plan := types.Plan{Name: "v6.6", Height: ctx.BlockHeight()}
	require.True(t, a.UpgradeKeeper.HasHandler(plan.Name))
	a.UpgradeKeeper.ApplyUpgrade(ctx, plan)
	after := a.UpgradeKeeper.GetModuleVersionMap(ctx)
	for _, name := range v66Modules {
		require.Equal(t, uint64(1), after[name], name)
	}
	require.Equal(t, layerxbridgetypes.DefaultAuthority(), a.LayerXBridgeKeeper.GetParams(ctx).Authority)
	require.Empty(t, a.LayerXBridgeKeeper.GetAttestorSet(ctx).Attestors)
	require.Empty(t, a.LayerXBridgeKeeper.GetCaps(ctx))
	launchpadParams := a.LaunchpadKeeper.GetParams(ctx)
	require.Equal(t, launchpadtypes.DefaultParams().QuoteDenom, launchpadParams.QuoteDenom)
	require.True(t, launchpadtypes.DefaultParams().CreationFee.Equal(launchpadParams.CreationFee))

	caller := a.AccountKeeper.GetModuleAddress(evmtypes.ModuleName)
	view := func(name, address, method string, args ...interface{}) []interface{} {
		info := precompiles.GetPrecompileInfo(name)
		require.Equal(t, common.HexToAddress(address), info.Address)
		input, err := info.ABI.Pack(method, args...)
		require.NoError(t, err)
		to := info.Address
		output, err := a.EvmKeeper.StaticCallEVM(ctx, caller, &to, input)
		require.NoError(t, err, "%s.%s", name, method)
		values, err := info.ABI.Unpack(method, output)
		require.NoError(t, err)
		return values
	}

	nonce := view(layerxexchange.PrecompileName, "0x0000000000000000000000000000000000001015",
		"intentNonce", common.HexToAddress("0x00000000000000000000000000000000000000a1"))
	require.Equal(t, uint64(0), nonce[0])

	attestors := view(layerxbridge.PrecompileName, "0x0000000000000000000000000000000000001016", "getAttestors")
	require.Empty(t, attestors[0])
	require.Empty(t, attestors[1])
	require.Equal(t, uint32(0), attestors[2])

	markets := view(launchpad.PrecompileName, "0x0000000000000000000000000000000000001017", "getMarketCount")
	require.Equal(t, 0, markets[0].(*big.Int).Sign())
}
