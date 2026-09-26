package app

import (
	"bytes"
	"go/format"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"text/template"
	"time"

	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/modules/evm/state"
	evmtypes "github.com/sidiora-labs/paxeer-network/modules/evm/types"
	"github.com/sidiora-labs/paxeer-network/modules/xweb"
	xwebtypes "github.com/sidiora-labs/paxeer-network/modules/xweb/types"
	"github.com/sidiora-labs/paxeer-network/precompiles"
	xwebprecompile "github.com/sidiora-labs/paxeer-network/precompiles/xweb"
	"github.com/sidiora-labs/paxeer-network/sdk/crypto/keys/secp256k1"
	"github.com/sidiora-labs/paxeer-network/sdk/store/rootmulti"
	storetypes "github.com/sidiora-labs/paxeer-network/sdk/store/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	upgradetypes "github.com/sidiora-labs/paxeer-network/sdk/x/upgrade/types"
	"github.com/stretchr/testify/require"
	dbm "github.com/tendermint/tm-db"
)

func readVersionLines(t *testing.T, path string) []string {
	t.Helper()
	content, err := os.ReadFile(filepath.Clean(path))
	require.NoError(t, err)
	var lines []string
	for _, line := range strings.Split(string(content), "\n") {
		if line = strings.TrimSpace(line); line != "" {
			lines = append(lines, line)
		}
	}
	return lines
}

func TestXWebUpgradeIsRegisteredUnderTheLatestTag(t *testing.T) {
	tags, err := f.ReadFile("tags")
	require.NoError(t, err)
	names := parseUpgradesList(string(tags))
	require.Equal(t, "v6.8", xwebUpgrade)
	require.Equal(t, precompiles.XWebUpgrade, xwebUpgrade)
	require.Equal(t, xwebUpgrade, names[len(names)-1])
	require.Equal(t, xwebUpgrade, LatestUpgrade)
	require.Less(t, indexOf(names, sidioraFeeTokenUpgrade), indexOf(names, xwebUpgrade))

	versions := readVersionLines(t, filepath.Join("..", "precompiles", "xweb", "versions"))
	require.Equal(t, []string{xwebUpgrade}, versions)

	testWrapper := NewTestWrapper(t, time.Now().UTC(), secp256k1.GenPrivKey().PubKey(), false)
	a := testWrapper.App
	a.RegisterUpgradeHandlers()
	require.True(t, a.UpgradeKeeper.HasHandler(xwebUpgrade))
	require.True(t, a.UpgradeKeeper.HasHandler(sidioraFeeTokenUpgrade))
}

func indexOf(names []string, name string) int {
	for i, candidate := range names {
		if candidate == name {
			return i
		}
	}
	return -1
}

// The generator's own template, fed the versions file the way
// scripts/bump_version feeds it, reproduces the committed setup.go byte for
// byte, so regenerating it produces no diff.
func TestXWebUpgradeSetupRegenerationProducesNoDiff(t *testing.T) {
	type legacyVersion struct {
		Tag    string
		Folder string
	}
	versions := readVersionLines(t, filepath.Join("..", "precompiles", "xweb", "versions"))
	require.NotEmpty(t, versions)
	legacy := make([]legacyVersion, 0, len(versions)-1)
	for _, tag := range versions[:len(versions)-1] {
		legacy = append(legacy, legacyVersion{Tag: tag, Folder: strings.ReplaceAll(tag, ".", "")})
	}
	tmpl, err := template.ParseFiles(filepath.Join("..", "scripts", "bump_version", "setup.go.tmpl"))
	require.NoError(t, err)
	var rendered bytes.Buffer
	require.NoError(t, tmpl.Execute(&rendered, struct {
		PackageName    string
		LegacyVersions []legacyVersion
	}{PackageName: "xweb", LegacyVersions: legacy}))
	generated, err := format.Source(rendered.Bytes())
	require.NoError(t, err)
	committed, err := os.ReadFile(filepath.Join("..", "precompiles", "xweb", "setup.go"))
	require.NoError(t, err)
	require.Equal(t, string(generated), string(committed))
}

func TestXWebUpgradeStoreLoaderAddsTheXWebStore(t *testing.T) {
	upgrades, ok := layerxStoreUpgrades(xwebUpgrade)
	require.True(t, ok)
	require.Equal(t, v68StoreUpgrades(), upgrades)
	require.Equal(t, []string{xwebtypes.StoreKey}, upgrades.Added)
	require.Empty(t, upgrades.Deleted)
	require.Empty(t, upgrades.Renamed)
	_, ok = layerxStoreUpgrades(sidioraFeeTokenUpgrade)
	require.False(t, ok)
	require.Contains(t, kvStoreKeyNames, xwebtypes.StoreKey)

	db := dbm.NewMemDB()
	below := rootmulti.NewStore(db)
	below.SetPruning(storetypes.PruneNothing)
	var bank *sdk.KVStoreKey
	for _, name := range kvStoreKeyNames {
		if name == xwebtypes.StoreKey {
			continue
		}
		key := sdk.NewKVStoreKey(name)
		if name == "bank" {
			bank = key
		}
		below.MountStoreWithDB(key, storetypes.StoreTypeIAVL, nil)
	}
	require.NoError(t, below.LoadLatestVersion())
	below.GetKVStore(bank).Set([]byte("balance"), []byte("kept"))
	require.Equal(t, int64(1), below.Commit(true).Version)

	upgraded := rootmulti.NewStore(db)
	upgraded.SetPruning(storetypes.PruneNothing)
	mounted := map[string]*sdk.KVStoreKey{}
	for _, name := range kvStoreKeyNames {
		key := sdk.NewKVStoreKey(name)
		mounted[name] = key
		upgraded.MountStoreWithDB(key, storetypes.StoreTypeIAVL, nil)
	}
	require.NoError(t, upgraded.LoadLatestVersionAndUpgrade(&upgrades))
	require.Equal(t, []byte("kept"), upgraded.GetKVStore(mounted["bank"]).Get([]byte("balance")))
	store := upgraded.GetKVStore(mounted[xwebtypes.StoreKey])
	require.NotNil(t, store)
	iter := store.Iterator(nil, nil)
	require.False(t, iter.Valid())
	require.NoError(t, iter.Close())
	require.Equal(t, int64(2), upgraded.Commit(true).Version)
}

func TestXWebUpgradeCustomPrecompileSetBelowTheUpgradeHoldsNoXWebEntry(t *testing.T) {
	address := common.HexToAddress(xwebtypes.PrecompileAddress)
	require.Equal(t, address, common.HexToAddress(xwebprecompile.XWebAddress))
	testWrapper := NewTestWrapper(t, time.Now().UTC(), secp256k1.GenPrivKey().PubKey(), false)
	keepers := testWrapper.App.GetPrecompileKeepers()

	below := precompiles.GetCustomPrecompiles(sidioraFeeTokenUpgrade, keepers)
	require.NotContains(t, below, address)
	require.NotEmpty(t, below)

	all := precompiles.GetCustomPrecompiles(xwebUpgrade, keepers)
	require.Contains(t, all, address)
	require.Len(t, all, len(below)+1)
	named, ok := all[address][xwebUpgrade].(precompiles.IPrecompile)
	require.True(t, ok)
	require.Equal(t, xwebprecompile.PrecompileName, named.GetName())

	dormant := xwebPrecompileSet(all, false)
	require.NotContains(t, dormant, address)
	require.Len(t, dormant, len(all)-1)
	for addr := range below {
		require.Contains(t, dormant, addr)
	}
	live := xwebPrecompileSet(all, true)
	require.Equal(t, all, live)
}

func TestXWebUpgradeSetsPausedDefaultsAndServesThePrecompileOnlyAfterIt(t *testing.T) {
	testWrapper := NewTestWrapper(t, time.Now().UTC(), secp256k1.GenPrivKey().PubKey(), true)
	a, ctx := testWrapper.App, testWrapper.Ctx
	address := common.HexToAddress(xwebtypes.PrecompileAddress)

	// The precompile keepers hand the precompile the application's keeper.
	keepers, ok := a.GetPrecompileKeepers().(*PrecompileKeepers)
	require.True(t, ok)
	require.Same(t, &a.XWebKeeper, keepers.XWebK())

	// A chain whose genesis initialised xweb serves it from the start.
	require.True(t, a.xwebLive(ctx))
	require.Contains(t, a.EvmKeeper.CustomPrecompiles(ctx), address)

	// Rewind to a chain below the upgrade: no xweb state, no xweb version.
	xwebStore := ctx.KVStore(a.GetKey(xwebtypes.StoreKey))
	iter := xwebStore.Iterator(nil, nil)
	var stale [][]byte
	for ; iter.Valid(); iter.Next() {
		stale = append(stale, iter.Key())
	}
	require.NoError(t, iter.Close())
	require.NotEmpty(t, stale)
	for _, key := range stale {
		xwebStore.Delete(key)
	}
	ctx.KVStore(a.GetKey(upgradetypes.StoreKey)).Delete(append([]byte{upgradetypes.VersionMapByte}, xwebtypes.ModuleName...))
	require.NotContains(t, a.UpgradeKeeper.GetModuleVersionMap(ctx), xwebtypes.ModuleName)
	require.Zero(t, a.UpgradeKeeper.GetDoneHeight(ctx, xwebUpgrade))

	a.refreshXWebPrecompile(ctx)
	require.False(t, a.xwebLive(ctx))
	require.NotContains(t, a.EvmKeeper.CustomPrecompiles(ctx), address)
	require.Contains(t, a.EvmKeeper.CustomPrecompiles(ctx), common.HexToAddress("0x0000000000000000000000000000000000001018"))

	caller := a.AccountKeeper.GetModuleAddress(evmtypes.ModuleName)
	info := precompiles.GetPrecompileInfo(xwebprecompile.PrecompileName)
	require.Equal(t, address, info.Address)
	feeInput, err := info.ABI.Pack(xwebprecompile.FeeMethod)
	require.NoError(t, err)
	output, err := a.EvmKeeper.StaticCallEVM(ctx, caller, &address, feeInput)
	require.NoError(t, err)
	require.Empty(t, output, "below the upgrade 0x…1019 is an empty account")

	a.RegisterUpgradeHandlers()
	plan := upgradetypes.Plan{Name: xwebUpgrade, Height: ctx.BlockHeight()}
	require.True(t, a.UpgradeKeeper.HasHandler(plan.Name))
	a.UpgradeKeeper.ApplyUpgrade(ctx, plan)

	require.Equal(t, xweb.AppModule{}.ConsensusVersion(), a.UpgradeKeeper.GetModuleVersionMap(ctx)[xwebtypes.ModuleName])
	require.Equal(t, ctx.BlockHeight(), a.UpgradeKeeper.GetDoneHeight(ctx, xwebUpgrade))

	defaults := xwebtypes.DefaultParams(xwebtypes.DefaultAuthority())
	params := a.XWebKeeper.GetParams(ctx)
	require.Equal(t, defaults.Authority, params.Authority)
	require.True(t, defaults.Fee.Equal(params.Fee))
	require.Equal(t, defaults.MaxPayloadBytes, params.MaxPayloadBytes)
	require.Equal(t, defaults.MaxCallbackGas, params.MaxCallbackGas)
	require.Equal(t, defaults.TimeoutBlocks, params.TimeoutBlocks)
	require.True(t, a.xwebInitialised(ctx))
	require.True(t, a.XWebKeeper.IsPaused(ctx))
	require.Empty(t, a.XWebKeeper.GetAttestorSet(ctx).Attestors)
	require.Zero(t, a.XWebKeeper.Threshold(ctx))
	require.Zero(t, a.XWebKeeper.Nonce(ctx))
	require.Empty(t, a.XWebKeeper.GetRequests(ctx))
	require.Empty(t, a.XWebKeeper.GetResults(ctx))

	require.True(t, a.xwebLive(ctx))
	require.Contains(t, a.EvmKeeper.CustomPrecompiles(ctx), address)
	output, err = a.EvmKeeper.StaticCallEVM(ctx, caller, &address, feeInput)
	require.NoError(t, err)
	values, err := info.ABI.Unpack(xwebprecompile.FeeMethod, output)
	require.NoError(t, err)
	require.Len(t, values, 1)
	require.Equal(t, defaults.Fee.Mul(state.SdkUhpxToSweiMultiplier).BigInt(), values[0])

	thresholdInput, err := info.ABI.Pack(xwebprecompile.ThresholdMethod)
	require.NoError(t, err)
	output, err = a.EvmKeeper.StaticCallEVM(ctx, caller, &address, thresholdInput)
	require.NoError(t, err)
	values, err = info.ABI.Unpack(xwebprecompile.ThresholdMethod, output)
	require.NoError(t, err)
	require.Equal(t, uint32(0), values[0])

	// A node restarted past the upgrade height serves it again from state.
	a.setXWebPrecompile(false)
	require.NotContains(t, a.EvmKeeper.CustomPrecompiles(ctx), address)
	a.refreshXWebPrecompile(ctx)
	require.Contains(t, a.EvmKeeper.CustomPrecompiles(ctx), address)
}

// The upgrade never overwrites xweb state that already exists: an unpaused
// module stays unpaused across the handler.
func TestXWebUpgradeKeepsExistingXWebState(t *testing.T) {
	testWrapper := NewTestWrapper(t, time.Now().UTC(), secp256k1.GenPrivKey().PubKey(), true)
	a, ctx := testWrapper.App, testWrapper.Ctx
	require.True(t, a.xwebInitialised(ctx))

	genesis := a.XWebKeeper.ExportGenesis(ctx)
	genesis.Paused = false
	a.XWebKeeper.InitGenesis(ctx, genesis)
	require.False(t, a.XWebKeeper.IsPaused(ctx))

	a.UpgradeKeeper.ApplyUpgrade(ctx, upgradetypes.Plan{Name: xwebUpgrade, Height: ctx.BlockHeight()})
	require.False(t, a.XWebKeeper.IsPaused(ctx))
	require.Equal(t, xweb.AppModule{}.ConsensusVersion(), a.UpgradeKeeper.GetModuleVersionMap(ctx)[xwebtypes.ModuleName])
	require.Contains(t, a.EvmKeeper.CustomPrecompiles(ctx), common.HexToAddress(xwebtypes.PrecompileAddress))
}
