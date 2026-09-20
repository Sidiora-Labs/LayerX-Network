package keys

import "fmt"

// Cosmos-SDK module store keys mounted on the memiavl backend in default
// production deployments. Defined as raw string literals (rather than
// re-exporting from x/* packages) to keep this package free of the heavy
// cosmos-sdk / ibc-go / wasmd / go-ethereum dependency closure.
//
// These string values are immutable on-disk format markers; changing any
// of them would break existing state.
const (
	AuthStoreKey         = "acc"          // cosmos/x/auth/types.StoreKey
	AuthzStoreKey        = "authz"        // cosmos/x/authz/keeper.StoreKey
	BankStoreKey         = "bank"         // cosmos/x/bank/types.StoreKey
	StakingStoreKey      = "staking"      // cosmos/x/staking/types.StoreKey
	MintStoreKey         = "mint"         // modules/mint/types.StoreKey
	DistributionStoreKey = "distribution" // cosmos/x/distribution/types.StoreKey
	SlashingStoreKey     = "slashing"     // cosmos/x/slashing/types.StoreKey
	GovStoreKey          = "gov"          // cosmos/x/gov/types.StoreKey
	ParamsStoreKey       = "params"       // cosmos/x/params/types.StoreKey
	IBCStoreKey          = "ibc"          // ibc-go/modules/core/24-host.StoreKey
	UpgradeStoreKey      = "upgrade"      // cosmos/x/upgrade/types.StoreKey
	FeegrantStoreKey     = "feegrant"     // cosmos/x/feegrant.StoreKey
	EvidenceStoreKey     = "evidence"     // cosmos/x/evidence/types.StoreKey
	IBCTransferStoreKey  = "transfer"     // ibc-go/modules/apps/transfer/types.StoreKey
	CapabilityStoreKey   = "capability"   // cosmos/x/capability/types.StoreKey
	OracleStoreKey       = "oracle"       // modules/oracle/types.StoreKey
	EVMStoreKey          = "evm"          // modules/evm/types.StoreKey
	WasmStoreKey         = "wasm"         // wasm/x/wasm/types.StoreKey
	EpochStoreKey        = "epoch"        // modules/epoch/types.StoreKey
	TokenfactoryStoreKey = "tokenfactory" // modules/tokenfactory/types.StoreKey
)

// MemIAVLStoreKeys is the canonical list of module KV store keys that are
// mounted on the memiavl backend in a default production deployment.
// It mirrors the slice passed to sdk.NewKVStoreKeys in app.New (see
// node/app.go). Keep this list in sync with that call site.
var MemIAVLStoreKeys = []string{
	AuthStoreKey,
	AuthzStoreKey,
	BankStoreKey,
	StakingStoreKey,
	MintStoreKey,
	DistributionStoreKey,
	SlashingStoreKey,
	GovStoreKey,
	ParamsStoreKey,
	IBCStoreKey,
	UpgradeStoreKey,
	FeegrantStoreKey,
	EvidenceStoreKey,
	IBCTransferStoreKey,
	CapabilityStoreKey,
	OracleStoreKey,
	EVMStoreKey,
	WasmStoreKey,
	EpochStoreKey,
	TokenfactoryStoreKey,
}

// memIAVLStoreKeySet is MemIAVLStoreKeys materialized as a set for O(1)
// membership checks. Populated in init().
var memIAVLStoreKeySet map[string]struct{}

func init() {
	memIAVLStoreKeySet = make(map[string]struct{}, len(MemIAVLStoreKeys))
	for _, k := range MemIAVLStoreKeys {
		memIAVLStoreKeySet[k] = struct{}{}
	}
}

// IsMemIAVLStoreKey reports whether name is a member of MemIAVLStoreKeys.
func IsMemIAVLStoreKey(name string) bool {
	_, ok := memIAVLStoreKeySet[name]
	return ok
}

// AllModulesExcept returns a list of modules excluding the specified modules.
// Returns an error if an excluded module is not a part of MemIAVLStoreKeys.
// The returned slice is safe to modify.
func AllModulesExcept(modulesNotToInclude ...string) ([]string, error) {
	exclude := make(map[string]bool, len(modulesNotToInclude))
	for _, m := range modulesNotToInclude {
		if !IsMemIAVLStoreKey(m) {
			return nil, fmt.Errorf("module %q is not a member of MemIAVLStoreKeys", m)
		}
		exclude[m] = true
	}

	result := make([]string, 0, len(MemIAVLStoreKeys)-len(exclude))
	for _, k := range MemIAVLStoreKeys {
		if !exclude[k] {
			result = append(result, k)
		}
	}
	return result, nil
}
