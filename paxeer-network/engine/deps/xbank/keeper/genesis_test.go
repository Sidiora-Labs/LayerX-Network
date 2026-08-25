package keeper_test

import (
	"github.com/sidiora-labs/paxeer-network/engine/deps/xbank/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

func (suite *IntegrationTestSuite) getTestBalancesAndSupply() ([]types.Balance, sdk.Coins) {
	addr1, _ := sdk.AccAddressFromBech32("pax10xwrnrezdg227cgt82az7f7j47q3zklvdh3g0l")
	addr2, _ := sdk.AccAddressFromBech32("pax1rs8v2232uv5nw8c88ruvyjy08mmxfx25sl0l5k")
	addr1Balance := sdk.Coins{sdk.NewInt64Coin("testcoin3", 10)}
	addr2Balance := sdk.Coins{sdk.NewInt64Coin("testcoin1", 32), sdk.NewInt64Coin("testcoin2", 34), sdk.NewInt64Coin(sdk.DefaultBondDenom, 2)}

	totalSupply := addr1Balance
	totalSupply = totalSupply.Add(addr2Balance...)

	return []types.Balance{
		{Address: addr2.String(), Coins: addr2Balance},
		{Address: addr1.String(), Coins: addr1Balance},
	}, totalSupply
}

func (suite *IntegrationTestSuite) TestInitGenesis() {
	m := types.Metadata{Description: sdk.DefaultBondDenom, Base: sdk.DefaultBondDenom, Display: sdk.DefaultBondDenom}
	g := types.DefaultGenesisState()
	g.DenomMetadata = []types.Metadata{m}
	bk := suite.app.GigaBankKeeper
	bk.InitGenesis(suite.ctx, g)

	m2, found := bk.GetDenomMetaData(suite.ctx, m.Base)
	suite.Require().True(found)
	suite.Require().Equal(m, m2)
}
