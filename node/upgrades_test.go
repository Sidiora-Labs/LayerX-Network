package app

import (
	"reflect"
	"testing"
	"time"

	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/modules/evm"
	evmtypes "github.com/sidiora-labs/paxeer-network/modules/evm/types"
	layerxbridgetypes "github.com/sidiora-labs/paxeer-network/modules/layerxbridge/types"
	"github.com/sidiora-labs/paxeer-network/sdk/crypto/keys/secp256k1"
	paramstypes "github.com/sidiora-labs/paxeer-network/sdk/x/params/types"
	upgradetypes "github.com/sidiora-labs/paxeer-network/sdk/x/upgrade/types"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestOverrideList(t *testing.T) {
	defaultList := upgradesList
	tests := []struct {
		name         string
		envValue     string
		expectedList []string
	}{
		{
			name:         "UPGRADE_VERSION_LIST not set",
			envValue:     "",
			expectedList: defaultList,
		},
		{
			name:         "UPGRADE_VERSION_LIST set with single value",
			envValue:     "2.0.0",
			expectedList: []string{"2.0.0"},
		},
		{
			name:         "UPGRADE_VERSION_LIST set with multiple values",
			envValue:     "2.0.0,2.1.0,2.2.0",
			expectedList: []string{"2.0.0", "2.1.0", "2.2.0"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if tt.envValue != "" {
				t.Setenv("UPGRADE_VERSION_LIST", tt.envValue)
			}
			// reset upgrades list before each test
			upgradesList = defaultList

			overrideList()

			assert.True(t, reflect.DeepEqual(tt.expectedList, upgradesList), "Expected %v but got %v", tt.expectedList, upgradesList)
		})
	}
}

func TestParseUpgradesList(t *testing.T) {
	tests := []struct {
		name     string
		input    string
		expected []string
	}{
		{
			name:     "empty string",
			input:    "",
			expected: []string{},
		},
		{
			name:  "comma separated",
			input: "v3.0.0,v1.0.2beta,v2.0.29beta",
			expected: []string{
				"v1.0.2beta",
				"v2.0.29beta",
				"v3.0.0",
			},
		},
		{
			name:  "comma separated double digit",
			input: "v3.11.0,v3.0.0,v1.0.2beta,v2.0.29beta,,v3.10.0",
			expected: []string{
				"v1.0.2beta",
				"v2.0.29beta",
				"v3.0.0",
				"v3.10.0",
				"v3.11.0",
			},
		},
		{
			name:  "newline separated",
			input: "v3.0.0\nv1.0.2beta\nv2.0.29beta",
			expected: []string{
				"v1.0.2beta",
				"v2.0.29beta",
				"v3.0.0",
			},
		},
		{
			name:  "mixed comma and newline separators",
			input: "v3.0.0,v1.0.2beta\nv2.0.29beta",
			expected: []string{
				"v1.0.2beta",
				"v2.0.29beta",
				"v3.0.0",
			},
		},
		{
			name:  "consecutive separators are ignored",
			input: "v3.0.0,,\n\nv1.0.2beta,\n,v2.0.29beta",
			expected: []string{
				"v1.0.2beta",
				"v2.0.29beta",
				"v3.0.0",
			},
		},
		{
			name:  "already sorted input stays sorted",
			input: "1.0.2beta,1.0.3beta,1.0.4beta",
			expected: []string{
				"1.0.2beta",
				"1.0.3beta",
				"1.0.4beta",
			},
		},
		{
			name:  "reverse sorted input gets sorted",
			input: "v6.4.0,v5.0.0,v3.0.0,1.0.2beta",
			expected: []string{
				"1.0.2beta",
				"v3.0.0",
				"v5.0.0",
				"v6.4.0",
			},
		},
		{
			name:  "prerelease tags sort before release",
			input: "v4.0.0-evm-devnet,v3.9.0,v4.0.1-evm-devnet",
			expected: []string{
				"v3.9.0",
				"v4.0.0-evm-devnet",
				"v4.0.1-evm-devnet",
			},
		},
		{
			name:  "mixed v-prefix and no prefix",
			input: "v3.0.9,3.0.8,3.0.7",
			expected: []string{
				"3.0.7",
				"3.0.8",
				"v3.0.9",
			},
		},
		{
			name:  "postfix prerelease versions",
			input: "1.2.2beta-postfix,1.0.7beta-postfix,1.1.2beta-internal",
			expected: []string{
				"1.0.7beta-postfix",
				"1.1.2beta-internal",
				"1.2.2beta-postfix",
			},
		},
		{
			name:     "single entry",
			input:    "v6.4.0",
			expected: []string{"v6.4.0"},
		},
		{
			name:  "large mixed list newline separated",
			input: "v6.0.0\nv5.0.0\n1.0.2beta\nv3.0.0\n2.0.29beta\nv4.0.0-evm-devnet\n1.1.0beta",
			expected: []string{
				"1.0.2beta",
				"1.1.0beta",
				"2.0.29beta",
				"v3.0.0",
				"v4.0.0-evm-devnet",
				"v5.0.0",
				"v6.0.0",
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := parseUpgradesList(tt.input)
			if len(got) == 0 && len(tt.expected) == 0 {
				return
			}
			if !reflect.DeepEqual(got, tt.expected) {
				t.Errorf("parseUpgradesList(%q)\n  got:  %v\n  want: %v", tt.input, got, tt.expected)
			}
		})
	}
}

// sidioraFeeTokenParamKeys are the five x/evm parameters the fee-token upgrade
// adds through the x/evm migration it runs.
var sidioraFeeTokenParamKeys = [][]byte{
	evmtypes.KeyFeeTokenEnabled,
	evmtypes.KeyAllowedFeeDenoms,
	evmtypes.KeyMaxFeeTokenSpread,
	evmtypes.KeyMaxFeeTokenRateAge,
	evmtypes.KeyFeeTokenDistribution,
}

func TestSidioraFeeTokenUpgradeIsRegisteredAndRunsTheFeeTokenMigration(t *testing.T) {
	tags, err := f.ReadFile("tags")
	require.NoError(t, err)
	names := parseUpgradesList(string(tags))
	require.Contains(t, names, sidioraFeeTokenUpgrade)
	require.Equal(t, sidioraFeeTokenUpgrade, names[len(names)-2])
	require.Equal(t, xwebUpgrade, names[len(names)-1])
	require.Equal(t, xwebUpgrade, LatestUpgrade)

	valPub := secp256k1.GenPrivKey().PubKey()
	testWrapper := NewTestWrapper(t, time.Now().UTC(), valPub, false)
	a, ctx := testWrapper.App, testWrapper.Ctx

	a.RegisterUpgradeHandlers()
	require.True(t, a.UpgradeKeeper.HasHandler(sidioraFeeTokenUpgrade))

	// The upgrade reads the chain of the registered Sidiora remote asset, which
	// is the deployment prerequisite of the plan.
	const chainID = uint64(1)
	require.NoError(t, a.LayerXBridgeKeeper.RegisterChain(ctx, layerxbridgetypes.MsgRegisterChain{
		Authority: layerxbridgetypes.DefaultAuthority(),
		Chain: layerxbridgetypes.Chain{
			ChainID:       chainID,
			Vault:         layerxbridgetypes.Address20(common.HexToAddress("0x00000000000000000000000000000000000000c1")),
			FinalityDepth: 64,
			Enabled:       true,
		},
	}))
	denom := layerxbridgetypes.SidioraDenom()
	registered, err := a.LayerXBridgeKeeper.EnsureSidioraDenom(ctx, chainID)
	require.NoError(t, err)
	require.Equal(t, denom, registered)

	// Rewind x/evm to the store and the module version a chain that predates the
	// fee token carries.
	paramsStore := ctx.KVStore(a.GetKey(paramstypes.StoreKey))
	for _, key := range sidioraFeeTokenParamKeys {
		paramsStore.Delete(append([]byte(evmtypes.ModuleName+"/"), key...))
		require.False(t, a.EvmKeeper.Paramstore.Has(ctx, key), string(key))
	}
	fromVersion := evm.AppModule{}.ConsensusVersion() - 1
	versions := a.UpgradeKeeper.GetModuleVersionMap(ctx)
	versions[evmtypes.ModuleName] = fromVersion
	a.UpgradeKeeper.SetModuleVersionMap(ctx, versions)

	a.UpgradeKeeper.ApplyUpgrade(ctx, upgradetypes.Plan{Name: sidioraFeeTokenUpgrade, Height: ctx.BlockHeight()})

	after := a.UpgradeKeeper.GetModuleVersionMap(ctx)
	require.Equal(t, fromVersion+1, after[evmtypes.ModuleName])
	require.Equal(t, evm.AppModule{}.ConsensusVersion(), after[evmtypes.ModuleName])

	for _, key := range sidioraFeeTokenParamKeys {
		require.True(t, a.EvmKeeper.Paramstore.Has(ctx, key), string(key))
	}
	params := a.EvmKeeper.GetParams(ctx)
	require.Equal(t, evmtypes.DefaultFeeTokenEnabled, params.FeeTokenEnabled)
	require.Empty(t, params.AllowedFeeDenoms)
	require.Equal(t, evmtypes.DefaultMaxFeeTokenSpread, params.MaxFeeTokenSpread)
	require.Equal(t, evmtypes.DefaultMaxFeeTokenRateAge, params.MaxFeeTokenRateAge)
	require.Equal(t, evmtypes.DefaultFeeTokenDistribution, params.FeeTokenDistribution)

	// The denom, its metadata and its remote asset record stand after the plan.
	record, found := a.LayerXBridgeKeeper.GetAssetByDenom(ctx, denom)
	require.True(t, found)
	require.Equal(t, chainID, record.ChainID)
	require.Equal(t, denom, record.Denom)
	metadata, found := a.BankKeeper.GetDenomMetaData(ctx, denom)
	require.True(t, found)
	require.NoError(t, metadata.Validate())
	require.Equal(t, denom, metadata.Base)
	require.Equal(t, layerxbridgetypes.SidioraSymbol, metadata.Symbol)
	require.Equal(t, layerxbridgetypes.SidioraSymbol, metadata.Display)
}

func TestSidioraFeeTokenUpgradeFailsWithoutTheRemoteAssetRegistration(t *testing.T) {
	valPub := secp256k1.GenPrivKey().PubKey()
	testWrapper := NewTestWrapper(t, time.Now().UTC(), valPub, false)
	a, ctx := testWrapper.App, testWrapper.Ctx

	a.RegisterUpgradeHandlers()
	require.True(t, a.UpgradeKeeper.HasHandler(sidioraFeeTokenUpgrade))
	denom := layerxbridgetypes.SidioraDenom()
	_, found := a.LayerXBridgeKeeper.GetAssetByDenom(ctx, denom)
	require.False(t, found)

	var recovered interface{}
	func() {
		defer func() { recovered = recover() }()
		a.UpgradeKeeper.ApplyUpgrade(ctx, upgradetypes.Plan{Name: sidioraFeeTokenUpgrade, Height: ctx.BlockHeight()})
	}()
	err, ok := recovered.(error)
	require.True(t, ok, "%v", recovered)
	require.ErrorContains(t, err, denom)

	_, found = a.BankKeeper.GetDenomMetaData(ctx, denom)
	require.False(t, found)
}
