package keeper_test

import (
	gocontext "context"
	"testing"

	"github.com/sidiora-labs/paxeer-network/modules/mint/keeper"

	"github.com/sidiora-labs/paxeer-network/node"

	tmproto "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/types"
	"github.com/stretchr/testify/suite"

	"github.com/sidiora-labs/paxeer-network/modules/mint/types" // TODO: Replace this with pax-chain. Leaving it for now otherwise tests fail
	"github.com/sidiora-labs/paxeer-network/sdk/baseapp"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

type MintTestSuite struct {
	suite.Suite

	app         *app.App
	ctx         sdk.Context
	queryClient types.QueryClient
}

func (suite *MintTestSuite) SetupTest() {
	app := app.Setup(suite.T(), false, false, false)
	ctx := app.BaseApp.NewContext(false, tmproto.Header{})

	queryHelper := baseapp.NewQueryServerTestHelper(ctx, app.InterfaceRegistry())

	types.RegisterQueryServer(queryHelper, keeper.NewQuerier(app.MintKeeper))
	queryClient := types.NewQueryClient(queryHelper)

	suite.app = app
	suite.ctx = ctx

	suite.queryClient = queryClient
}

func (suite *MintTestSuite) TestGRPCParams() {
	queryClient := suite.queryClient

	_, err := queryClient.Params(gocontext.Background(), &types.QueryParamsRequest{})
	suite.Require().NoError(err)

	_, err = queryClient.Minter(gocontext.Background(), &types.QueryMinterRequest{})
	suite.Require().NoError(err)
}

func TestMintTestSuite(t *testing.T) {
	suite.Run(t, new(MintTestSuite))
}
