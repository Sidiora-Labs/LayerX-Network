package utils

import (
	"fmt"

	ibctransfertypes "github.com/sidiora-labs/paxeer-network/interchain/modules/apps/transfer/types"
	ibchost "github.com/sidiora-labs/paxeer-network/interchain/modules/core/24-host"
	epochmoduletypes "github.com/sidiora-labs/paxeer-network/modules/epoch/types"
	evmtypes "github.com/sidiora-labs/paxeer-network/modules/evm/types"
	minttypes "github.com/sidiora-labs/paxeer-network/modules/mint/types"
	oracletypes "github.com/sidiora-labs/paxeer-network/modules/oracle/types"
	tokenfactorytypes "github.com/sidiora-labs/paxeer-network/modules/tokenfactory/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	authtypes "github.com/sidiora-labs/paxeer-network/sdk/x/auth/types"
	authzkeeper "github.com/sidiora-labs/paxeer-network/sdk/x/authz/keeper"
	banktypes "github.com/sidiora-labs/paxeer-network/sdk/x/bank/types"
	capabilitytypes "github.com/sidiora-labs/paxeer-network/sdk/x/capability/types"
	distrtypes "github.com/sidiora-labs/paxeer-network/sdk/x/distribution/types"
	evidencetypes "github.com/sidiora-labs/paxeer-network/sdk/x/evidence/types"
	"github.com/sidiora-labs/paxeer-network/sdk/x/feegrant"
	govtypes "github.com/sidiora-labs/paxeer-network/sdk/x/gov/types"
	paramstypes "github.com/sidiora-labs/paxeer-network/sdk/x/params/types"
	slashingtypes "github.com/sidiora-labs/paxeer-network/sdk/x/slashing/types"
	stakingtypes "github.com/sidiora-labs/paxeer-network/sdk/x/staking/types"
	upgradetypes "github.com/sidiora-labs/paxeer-network/sdk/x/upgrade/types"
	"github.com/sidiora-labs/paxeer-network/wasm/x/wasm"
)

var ModuleKeys = sdk.NewKVStoreKeys(
	authtypes.StoreKey, authzkeeper.StoreKey, banktypes.StoreKey, stakingtypes.StoreKey,
	minttypes.StoreKey, distrtypes.StoreKey, slashingtypes.StoreKey,
	govtypes.StoreKey, paramstypes.StoreKey, ibchost.StoreKey, upgradetypes.StoreKey, feegrant.StoreKey,
	evidencetypes.StoreKey, ibctransfertypes.StoreKey, capabilitytypes.StoreKey, oracletypes.StoreKey,
	evmtypes.StoreKey, wasm.StoreKey, epochmoduletypes.StoreKey, tokenfactorytypes.StoreKey,
)

var Modules = []string{
	"authz",
	"acc",
	"bank",
	"capability",
	"distribution",
	"epoch",
	"evidence",
	"evm",
	"feegrant",
	"gov",
	"ibc",
	"mint",
	"oracle",
	"params",
	"slashing",
	"staking",
	"tokenfactory",
	"transfer",
	"upgrade",
	"wasm"}

func BuildRawPrefix(moduleName string) string {
	return fmt.Sprintf("s/k:%s/n", moduleName)
}

func BuildTreePrefix(moduleName string) string {
	return fmt.Sprintf("s/k:%s/", moduleName)
}
