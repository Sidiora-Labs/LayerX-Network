package keeper_test

import (
	"testing"

	"github.com/stretchr/testify/require"

	abci "github.com/sidiora-labs/paxeer-network/consensus/abci/types"
	tmproto "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/types"

	paxapp "github.com/sidiora-labs/paxeer-network/node"
	"github.com/sidiora-labs/paxeer-network/sdk/codec"
	"github.com/sidiora-labs/paxeer-network/sdk/x/slashing/keeper"
	"github.com/sidiora-labs/paxeer-network/sdk/x/slashing/testslashing"
	"github.com/sidiora-labs/paxeer-network/sdk/x/slashing/types"
)

func TestNewQuerier(t *testing.T) {
	app := paxapp.Setup(t, false, false, false)
	ctx := app.BaseApp.NewContext(false, tmproto.Header{})
	app.SlashingKeeper.SetParams(ctx, testslashing.TestParams())
	legacyQuerierCdc := codec.NewAminoCodec(app.LegacyAmino())
	querier := keeper.NewQuerier(app.SlashingKeeper, legacyQuerierCdc.LegacyAmino)

	query := abci.RequestQuery{
		Path: "",
		Data: []byte{},
	}

	_, err := querier(ctx, []string{types.QueryParameters}, query)
	require.NoError(t, err)
}

func TestQueryParams(t *testing.T) {
	cdc := codec.NewLegacyAmino()
	legacyQuerierCdc := codec.NewAminoCodec(cdc)
	app := paxapp.Setup(t, false, false, false)
	ctx := app.BaseApp.NewContext(false, tmproto.Header{})
	app.SlashingKeeper.SetParams(ctx, testslashing.TestParams())

	querier := keeper.NewQuerier(app.SlashingKeeper, legacyQuerierCdc.LegacyAmino)

	query := abci.RequestQuery{
		Path: "",
		Data: []byte{},
	}

	var params types.Params

	res, err := querier(ctx, []string{types.QueryParameters}, query)
	require.NoError(t, err)

	err = cdc.UnmarshalAsJSON(res, &params)
	require.NoError(t, err)
	require.Equal(t, app.SlashingKeeper.GetParams(ctx), params)
}
