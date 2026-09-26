package types_test

import (
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/modules/layerxbridge/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/stretchr/testify/require"
)

const otherChainID = uint64(1)

var (
	genesisVault = types.Address20{0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11}
	otherAsset   = types.Address20{0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44}
	sidiora      = types.Address20(common.HexToAddress(types.SidioraRemoteAddress))
)

// genesisWith registers Solana and chain 1 and carries assets, each with a
// cap and an in-flight amount on its denom.
func genesisWith(assets ...types.BridgedAsset) types.GenesisState {
	genesis := *types.DefaultGenesis()
	genesis.Chains = []types.Chain{
		{ChainID: types.SidioraHomeChainID, Vault: genesisVault, FinalityDepth: 32, Enabled: true},
		{ChainID: otherChainID, Vault: genesisVault, FinalityDepth: 64, Enabled: true},
	}
	genesis.Assets = assets
	for _, asset := range assets {
		genesis.Caps = append(genesis.Caps, types.Cap{Denom: asset.Denom, MaxInFlight: sdk.NewInt(1500), MaxPerTx: sdk.NewInt(1000)})
		genesis.InFlight = append(genesis.InFlight, types.InFlight{Denom: asset.Denom, Amount: sdk.NewInt(200)})
	}
	return genesis
}

func TestGenesisAcceptsTheSidioraPairWithTheUsidDenom(t *testing.T) {
	pair := types.BridgedAsset{ChainID: types.SidioraHomeChainID, Asset: sidiora, Denom: types.SidioraDenom()}
	require.True(t, types.IsSidioraPair(pair.ChainID, pair.Asset))
	require.Equal(t, types.SidioraDenom(), types.AssetDenom(pair.ChainID, pair.Asset))
	require.NoError(t, genesisWith(pair).Validate())
	require.NoError(t, genesisWith(pair, types.BridgedAsset{ChainID: otherChainID, Asset: otherAsset,
		Denom: types.Denom(otherChainID, otherAsset)}).Validate())
}

func TestGenesisRequiresTheDerivedDenomForEveryOtherAsset(t *testing.T) {
	for _, pair := range []struct {
		chainID uint64
		asset   types.Address20
	}{{otherChainID, otherAsset}, {otherChainID, sidiora}, {types.SidioraHomeChainID, otherAsset}} {
		require.False(t, types.IsSidioraPair(pair.chainID, pair.asset))
		derived := types.Denom(pair.chainID, pair.asset)
		require.Equal(t, derived, types.AssetDenom(pair.chainID, pair.asset))
		require.NoError(t, genesisWith(types.BridgedAsset{ChainID: pair.chainID, Asset: pair.asset, Denom: derived}).Validate(),
			"chain %d asset %s", pair.chainID, pair.asset.Hex())
		require.ErrorIs(t, genesisWith(types.BridgedAsset{ChainID: pair.chainID, Asset: pair.asset,
			Denom: types.Denom(pair.chainID+1, pair.asset)}).Validate(), types.ErrInvalidGenesis,
			"chain %d asset %s under another derived denom", pair.chainID, pair.asset.Hex())
	}
}

func TestGenesisRefusesEachCrossSubstitution(t *testing.T) {
	refused := map[string]types.BridgedAsset{
		"usid denom on another chain":         {ChainID: otherChainID, Asset: sidiora, Denom: types.SidioraDenom()},
		"usid denom for another Solana asset": {ChainID: types.SidioraHomeChainID, Asset: otherAsset, Denom: types.SidioraDenom()},
		"usid denom for another pair":         {ChainID: otherChainID, Asset: otherAsset, Denom: types.SidioraDenom()},
		"derived denom for the Sidiora pair": {ChainID: types.SidioraHomeChainID, Asset: sidiora,
			Denom: types.Denom(types.SidioraHomeChainID, sidiora)},
	}
	for name, asset := range refused {
		require.ErrorIs(t, genesisWith(asset).Validate(), types.ErrInvalidGenesis, name)
	}
}
