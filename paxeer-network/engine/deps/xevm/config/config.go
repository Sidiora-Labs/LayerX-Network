package config

import (
	canonical "github.com/sidiora-labs/paxeer-network/modules/evm/config"
	"math/big"

	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

const DefaultChainID = canonical.DefaultChainID

var ChainIDMapping = canonical.ChainIDMapping
var EVMChainIDMapping = canonical.EVMChainIDMapping

func GetEVMChainID(cosmosChainID string) *big.Int {
	if evmChainID, ok := ChainIDMapping[cosmosChainID]; ok {
		return big.NewInt(evmChainID)
	}
	return big.NewInt(DefaultChainID)
}

func GetVersionWthDefault(ctx sdk.Context, override uint16, defaultVersion uint16) uint16 {
	// overrides are only available on non-live chain IDs
	if override > 0 && !IsLiveChainID(ctx) {
		return override
	}
	return defaultVersion
}

// IsLiveChainID return true if one of the live chainIDs
func IsLiveChainID(ctx sdk.Context) bool {
	_, ok := ChainIDMapping[ctx.ChainID()]
	return ok
}

// IsLiveEVMChainID returns true is this chainID is reserved for one of the live chains.
func IsLiveEVMChainID(evmChainID int64) bool {
	_, ok := EVMChainIDMapping[evmChainID]
	return ok
}
