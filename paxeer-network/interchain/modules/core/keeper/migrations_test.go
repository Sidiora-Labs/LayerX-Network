package keeper_test

import (
	ibckeeper "github.com/sidiora-labs/paxeer-network/interchain/modules/core/keeper"
	"github.com/sidiora-labs/paxeer-network/interchain/modules/core/types"
	"github.com/sidiora-labs/paxeer-network/sdk/store/prefix"
	paramtypes "github.com/sidiora-labs/paxeer-network/sdk/x/params/types"
)

func (suite *KeeperTestSuite) TestMigrate2to3() {
	ctx := suite.chainA.GetContext()
	ibcKeeper := suite.chainA.App.GetIBCKeeper()

	paramStore := prefix.NewStore(
		ctx.KVStore(suite.chainA.GetSimApp().GetKey(paramtypes.StoreKey)),
		[]byte(ibcKeeper.GetParamSpace().Name()+"/"),
	)
	paramStore.Delete(types.KeyInboundEnabled)
	paramStore.Delete(types.KeyOutboundEnabled)

	suite.Require().Panics(func() {
		ibcKeeper.GetParams(ctx)
	})

	m := ibckeeper.NewMigrator(*ibcKeeper)
	suite.Require().NoError(m.Migrate2to3(ctx))
	suite.Require().Equal(types.DefaultParams(), ibcKeeper.GetParams(ctx))
}
