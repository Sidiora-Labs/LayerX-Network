package keeper_test

import (
	tmproto "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/types"
	banktypes "github.com/sidiora-labs/paxeer-network/sdk/x/bank/types"

	"github.com/sidiora-labs/paxeer-network/modules/tokenfactory/types"
)

func (suite *KeeperTestSuite) TestGenesis() {
	genesisState := types.GenesisState{
		FactoryDenoms: []types.GenesisDenom{
			{
				Denom: "factory/pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288/bitcoin",
				AuthorityMetadata: types.DenomAuthorityMetadata{
					Admin: "pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288",
				},
			},
			{
				Denom: "factory/pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288/diff-admin",
				AuthorityMetadata: types.DenomAuthorityMetadata{
					Admin: "pax1hjfwcza3e3uzeznf3qthhakdr9juetl7ee472u",
				},
			},
			{
				Denom: "factory/pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288/litecoin",
				AuthorityMetadata: types.DenomAuthorityMetadata{
					Admin: "pax1y3pxq5dp900czh0mkudhjdqjq5m8cpmmsnt288",
				},
			},
		},
	}
	app := suite.App
	suite.Ctx = app.BaseApp.NewContext(false, tmproto.Header{})
	// Test both with bank denom metadata set, and not set.
	for i, denom := range genesisState.FactoryDenoms {
		// hacky, sets bank metadata to exist if i != 0, to cover both cases.
		if i != 0 {
			app.BankKeeper.SetDenomMetaData(suite.Ctx, banktypes.Metadata{Base: denom.GetDenom()})
		}
	}

	app.TokenFactoryKeeper.InitGenesis(suite.Ctx, genesisState)
	exportedGenesis := app.TokenFactoryKeeper.ExportGenesis(suite.Ctx)
	suite.Require().NotNil(exportedGenesis)
	suite.Require().Equal(genesisState, *exportedGenesis)
}
