package keeper_test

import (
	"testing"
	"time"

	tmproto "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/types"
	"github.com/sidiora-labs/paxeer-network/node"
	"github.com/stretchr/testify/suite"

	"github.com/sidiora-labs/paxeer-network/sdk/crypto/keys/secp256k1"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/sdk/x/authz/keeper"
	bank "github.com/sidiora-labs/paxeer-network/sdk/x/bank/types"
)

type GenesisTestSuite struct {
	suite.Suite

	ctx    sdk.Context
	keeper keeper.Keeper
}

func (suite *GenesisTestSuite) SetupTest() {
	checkTx := false
	app := app.Setup(suite.T(), checkTx, false, false)

	suite.ctx = app.BaseApp.NewContext(checkTx, tmproto.Header{Height: 1})
	suite.keeper = app.AuthzKeeper
}

var (
	granteePub  = secp256k1.GenPrivKey().PubKey()
	granterPub  = secp256k1.GenPrivKey().PubKey()
	granteeAddr = sdk.AccAddress(granteePub.Address())
	granterAddr = sdk.AccAddress(granterPub.Address())
)

func (suite *GenesisTestSuite) TestImportExportGenesis() {
	coins := sdk.NewCoins(sdk.NewCoin("foo", sdk.NewInt(1_000)))

	now := suite.ctx.BlockHeader().Time
	grant := &bank.SendAuthorization{SpendLimit: coins}
	err := suite.keeper.SaveGrant(suite.ctx, granteeAddr, granterAddr, grant, now.Add(time.Hour))
	suite.Require().NoError(err)
	genesis := suite.keeper.ExportGenesis(suite.ctx)

	// Clear keeper
	suite.keeper.DeleteGrant(suite.ctx, granteeAddr, granterAddr, grant.MsgTypeURL())

	suite.keeper.InitGenesis(suite.ctx, genesis)
	newGenesis := suite.keeper.ExportGenesis(suite.ctx)
	suite.Require().Equal(genesis, newGenesis)
}

func TestGenesisTestSuite(t *testing.T) {
	suite.Run(t, new(GenesisTestSuite))
}
