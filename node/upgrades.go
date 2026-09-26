package app

import (
	"embed"
	"fmt"
	"os"
	"strings"

	"github.com/ethereum/go-ethereum/common"
	layerxbridgetypes "github.com/sidiora-labs/paxeer-network/modules/layerxbridge/types"
	"github.com/sidiora-labs/paxeer-network/modules/xweb"
	xwebtypes "github.com/sidiora-labs/paxeer-network/modules/xweb/types"
	"github.com/sidiora-labs/paxeer-network/precompiles"
	putils "github.com/sidiora-labs/paxeer-network/precompiles/utils"
	storetypes "github.com/sidiora-labs/paxeer-network/sdk/store/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/sdk/types/module"
	upgradetypes "github.com/sidiora-labs/paxeer-network/sdk/x/upgrade/types"
	"golang.org/x/mod/semver"
)

//go:embed tags
var f embed.FS

// NOTE: When performing upgrades, make sure to keep / register the handlers
// for both the current (n) and the previous (n-1) upgrade name. There is a bug
// in a missing value in a log statement for which the fix is not released
var upgradesList []string

var LatestUpgrade string

func init() {
	content, err := f.ReadFile("tags")
	if err != nil {
		panic(err)
	}
	upgradesList = parseUpgradesList(string(content))
	LatestUpgrade = upgradesList[len(upgradesList)-1]
}

func parseUpgradesList(list string) []string {
	upgrades := strings.FieldsFunc(list, func(r rune) bool {
		return r == '\n' || r == ','
	})
	// Upgrades names must be in alphabetical order
	// https://github.com/cosmos/cosmos-sdk/issues/11707
	semver.Sort(upgrades)
	return upgrades
}

// if there is an override list, use that instead, for integration tests
func overrideList() {
	// if there is an override list, use that instead, for integration tests
	envList := os.Getenv("UPGRADE_VERSION_LIST")
	if envList != "" {
		upgradesList = parseUpgradesList(envList)
	}
}

// sidioraFeeTokenUpgrade gates the Sidiora fee token: it runs the x/evm
// fee-token parameter migration and creates the Sidiora denom and its bank
// metadata under the bridge module account.
const sidioraFeeTokenUpgrade = "v6.7"

// xwebUpgrade adds the xweb store, initialises the xweb module paused with its
// documented default parameters and no attestor, and from its height on serves
// the xweb precompile.
const xwebUpgrade = precompiles.XWebUpgrade

func (app *App) RegisterUpgradeHandlers() {
	// if there is an override list, use that instead, for integration tests
	overrideList()
	for _, upgradeName := range upgradesList {
		app.UpgradeKeeper.SetUpgradeHandler(upgradeName, func(ctx sdk.Context, plan upgradetypes.Plan, fromVM module.VersionMap) (module.VersionMap, error) {
			// Set params to Distribution here when migrating
			if upgradeName == "1.2.3beta" {
				newVM, err := app.mm.RunMigrations(ctx, app.configurator, fromVM)
				if err != nil {
					return newVM, err
				}

				params := app.DistrKeeper.GetParams(ctx)
				params.CommunityTax = sdk.NewDec(0)
				app.DistrKeeper.SetParams(ctx, params)

				return newVM, err
			}

			if upgradeName == "v6.0.2" {
				newVM, err := app.mm.RunMigrations(ctx, app.configurator, fromVM)
				if err != nil {
					return newVM, err
				}

				cp := app.GetConsensusParams(ctx)
				cp.Block.MinTxsInBlock = 10
				app.StoreConsensusParams(ctx, cp)
				return newVM, err
			}

			if upgradeName == "v6.0.5" {
				newVM, err := app.mm.RunMigrations(ctx, app.configurator, fromVM)
				if err != nil {
					return newVM, err
				}

				cp := app.GetConsensusParams(ctx)
				cp.Block.MaxGasWanted = 50000000 // 50 mil
				app.StoreConsensusParams(ctx, cp)
				return newVM, err
			}

			if upgradeName == sidioraFeeTokenUpgrade {
				newVM, err := app.mm.RunMigrations(ctx, app.configurator, fromVM)
				if err != nil {
					return newVM, err
				}

				denom := layerxbridgetypes.SidioraDenom()
				asset, found := app.LayerXBridgeKeeper.GetAssetByDenom(ctx, denom)
				if !found {
					return newVM, fmt.Errorf("upgrade %s requires the registered Sidiora remote asset of %s", upgradeName, denom)
				}
				if _, err := app.LayerXBridgeKeeper.EnsureSidioraDenom(ctx, asset.ChainID); err != nil {
					return newVM, err
				}
				return newVM, nil
			}

			if upgradeName == xwebUpgrade {
				return app.runXWebUpgrade(ctx, fromVM)
			}

			return app.mm.RunMigrations(ctx, app.configurator, fromVM)
		})
	}
}

// runXWebUpgrade runs the module migrations with xweb taken as present, so the
// migrations never initialise it, then initialises it from the default genesis
// unless its state already exists, and serves the xweb precompile from this
// block on.
func (app *App) runXWebUpgrade(ctx sdk.Context, fromVM module.VersionMap) (module.VersionMap, error) {
	initialised := app.xwebInitialised(ctx)
	versions := make(module.VersionMap, len(fromVM)+1)
	for name, version := range fromVM {
		versions[name] = version
	}
	versions[xwebtypes.ModuleName] = xweb.AppModule{}.ConsensusVersion()
	newVM, err := app.mm.RunMigrations(ctx, app.configurator, versions)
	if err != nil {
		return newVM, err
	}
	if !initialised {
		genesis := xwebtypes.DefaultGenesis()
		if err := genesis.Validate(); err != nil {
			return newVM, fmt.Errorf("upgrade %s: %w", xwebUpgrade, err)
		}
		app.XWebKeeper.InitGenesis(ctx, *genesis)
	}
	app.setXWebPrecompile(true)
	return newVM, nil
}

// v68StoreUpgrades mounts the xweb store at the xweb upgrade height.
func v68StoreUpgrades() storetypes.StoreUpgrades {
	return storetypes.StoreUpgrades{
		Added: []string{xwebtypes.StoreKey},
	}
}

// xwebInitialised reports whether the xweb module has state: its genesis or the
// xweb upgrade stored its parameters.
func (app *App) xwebInitialised(ctx sdk.Context) bool {
	return ctx.KVStore(app.GetKey(xwebtypes.StoreKey)).Has(xwebtypes.ParamsKey)
}

// xwebLive reports whether the xweb module is live in the state of ctx: the
// xweb upgrade is done, or the chain started with xweb in its module versions
// and its genesis initialised it.
func (app *App) xwebLive(ctx sdk.Context) bool {
	if app.UpgradeKeeper.GetDoneHeight(ctx, xwebUpgrade) > 0 {
		return true
	}
	if _, known := app.UpgradeKeeper.GetModuleVersionMap(ctx)[xwebtypes.ModuleName]; !known {
		return false
	}
	return app.xwebInitialised(ctx)
}

// refreshXWebPrecompile serves the xweb precompile exactly when the xweb
// module is live in the state of ctx.
func (app *App) refreshXWebPrecompile(ctx sdk.Context) {
	app.setXWebPrecompile(app.xwebLive(ctx))
}

// setXWebPrecompile hands the EVM keeper the custom precompile set with or
// without the xweb entry. An application built without custom precompiles
// keeps none.
func (app *App) setXWebPrecompile(live bool) {
	if app.customPrecompiles == nil {
		return
	}
	app.EvmKeeper.SetCustomPrecompiles(xwebPrecompileSet(app.customPrecompiles, live), LatestUpgrade)
}

// xwebPrecompileSet copies the custom precompile set, leaving out the xweb
// entry unless the xweb module is live.
func xwebPrecompileSet(all map[common.Address]putils.VersionedPrecompiles, live bool) map[common.Address]putils.VersionedPrecompiles {
	address := common.HexToAddress(xwebtypes.PrecompileAddress)
	set := make(map[common.Address]putils.VersionedPrecompiles, len(all))
	for addr, versioned := range all {
		if addr == address && !live {
			continue
		}
		set[addr] = versioned
	}
	return set
}

const v606UpgradeHeight = 151573570
