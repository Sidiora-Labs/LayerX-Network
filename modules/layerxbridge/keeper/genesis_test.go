package keeper_test

import (
	"testing"

	bridgetestutil "github.com/sidiora-labs/paxeer-network/modules/layerxbridge/testutil"
	"github.com/sidiora-labs/paxeer-network/modules/layerxbridge/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/stretchr/testify/require"
)

func TestGenesisRoundTripsTheRegisteredSidioraPair(t *testing.T) {
	s := newSuite(t, true)
	require.NoError(t, s.k.RegisterChain(s.ctx, types.MsgRegisterChain{Authority: authority, Chain: solanaChain}))
	denom, err := s.k.RegisterSidioraPair(s.ctx, types.MsgRegisterSidioraPair{Authority: authority, ChainID: types.SidioraHomeChainID})
	require.NoError(t, err)
	require.Equal(t, types.SidioraDenom(), denom)
	require.NoError(t, s.k.SetCap(s.ctx, *sidioraCap(authority)))
	in := sidioraDeposit(1, 700)
	in.ChainID = types.SidioraHomeChainID
	_, err = s.k.BridgeIn(s.ctx, in, s.signed(in, s.attestors[0], s.attestors[1]))
	require.NoError(t, err)

	pair := types.BridgedAsset{ChainID: types.SidioraHomeChainID, Asset: sidioraAsset(), Denom: denom}
	limit := types.Cap{Denom: denom, MaxInFlight: sdk.NewInt(1500), MaxPerTx: sdk.NewInt(1000)}
	flying := types.InFlight{Denom: denom, Amount: sdk.NewInt(700)}
	exported := s.k.ExportGenesis(s.ctx)
	require.NoError(t, exported.Validate())
	require.Contains(t, exported.Assets, pair)
	require.Contains(t, exported.Caps, limit)
	require.Contains(t, exported.InFlight, flying)

	fresh, ctx := bridgetestutil.NewKeeper(s.app, s.ctx)
	fresh.InitGenesis(ctx, exported)
	record, found := fresh.GetAsset(ctx, types.SidioraHomeChainID, sidioraAsset())
	require.True(t, found)
	require.Equal(t, pair, record)
	reverse, found := fresh.GetAssetByDenom(ctx, denom)
	require.True(t, found)
	require.Equal(t, pair, reverse)
	imported, found := fresh.GetCap(ctx, denom)
	require.True(t, found)
	require.Equal(t, limit, imported)
	require.Equal(t, flying.Amount, fresh.InFlight(ctx, denom))
	require.Equal(t, exported, fresh.ExportGenesis(ctx))
}
