package keeper_test

import (
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"

	"github.com/sidiora-labs/paxeer-network/interchain/modules/apps/27-interchain-accounts/host/types"
)

func (suite *KeeperTestSuite) TestQueryParams() {
	ctx := sdk.WrapSDKContext(suite.chainA.GetContext())
	expParams := types.DefaultParams()
	res, _ := suite.chainA.GetSimApp().ICAHostKeeper.Params(ctx, &types.QueryParamsRequest{})
	suite.Require().Equal(&expParams, res.Params)
}
