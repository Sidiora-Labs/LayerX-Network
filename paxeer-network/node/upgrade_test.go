package app_test

import (
	"testing"
	"time"

	abci "github.com/sidiora-labs/paxeer-network/consensus/abci/types"
	tmproto "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/types"
	"github.com/sidiora-labs/paxeer-network/node"
	"github.com/sidiora-labs/paxeer-network/sdk/crypto/keys/secp256k1"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/sdk/x/upgrade/types"
	"github.com/stretchr/testify/require"
)

func TestUpgradesListIsSorted(t *testing.T) {
	tm := time.Now().UTC()
	valPub := secp256k1.GenPrivKey().PubKey()
	testWrapper := app.NewTestWrapper(t, tm, valPub, false)
	testWrapper.App.RegisterUpgradeHandlers()
}

// Test community tax param is set to 0 as part of upgrade 1.2.3beta
func TestDistributionCommunityTaxParamMigration(t *testing.T) {
	tm := time.Now().UTC()
	valPub := secp256k1.GenPrivKey().PubKey()
	testWrapper := app.NewTestWrapper(t, tm, valPub, false)
	testWrapper.App.RegisterUpgradeHandlers()
	params := testWrapper.App.DistrKeeper.GetParams(testWrapper.Ctx)
	testWrapper.Require().Equal(params.CommunityTax, sdk.NewDec(0))
}

func TestSkipOptimisticProcessingOnUpgrade(t *testing.T) {
	t.Parallel()

	t.Run("Test optimistic processing is skipped on upgrade", func(t *testing.T) {
		tm := time.Now().UTC()
		valPub := secp256k1.GenPrivKey().PubKey()
		testWrapper := app.NewTestWrapper(t, tm, valPub, false)

		// No optimistic processing with upgrade scheduled
		testCtx := testWrapper.App.BaseApp.NewContext(false, tmproto.Header{Height: 3, ChainID: "pax-test", Time: tm})

		testWrapper.App.UpgradeKeeper.ScheduleUpgrade(testWrapper.Ctx, types.Plan{
			Name:   "test-plan",
			Height: testCtx.BlockHeight(),
		})
		plan, found := testWrapper.App.UpgradeKeeper.GetUpgradePlan(testCtx)
		require.True(t, found)
		require.True(t, plan.ShouldExecute(testCtx))

		res, _ := testWrapper.App.ProcessProposalHandler(testCtx, &abci.RequestProcessProposal{
			Header: &tmproto.Header{Height: 1, ChainID: "pax-test"},
		})
		require.Equal(t, res.Status, abci.ResponseProcessProposal_ACCEPT)
		require.True(t, testWrapper.App.GetOptimisticProcessingInfo().Aborted)
	})

	t.Run("Test optimistic processing if no upgrade", func(t *testing.T) {
		tm := time.Now().UTC()
		valPub := secp256k1.GenPrivKey().PubKey()
		testWrapper := app.NewTestWrapper(t, tm, valPub, false)
		testCtx := testWrapper.App.BaseApp.NewContext(false, tmproto.Header{Height: 3, ChainID: "pax-test", Time: tm})

		testWrapper.App.UpgradeKeeper.ScheduleUpgrade(testWrapper.Ctx, types.Plan{
			Name:   "test-plan",
			Height: testCtx.BlockHeight() + 1,
		})
		plan, found := testWrapper.App.UpgradeKeeper.GetUpgradePlan(testCtx)
		require.True(t, found)
		require.False(t, plan.ShouldExecute(testCtx))

		go func() {
			testWrapper.App.ProcessProposalHandler(testCtx, &abci.RequestProcessProposal{Header: &tmproto.Header{Height: 1, ChainID: "pax-test"}})
		}()

		require.Eventually(t, func() bool {
			opi := testWrapper.App.GetOptimisticProcessingInfo()
			if opi.Completion == nil {
				return false
			}
			<-opi.Completion
			return true
		}, 5*time.Second, time.Millisecond*100)

		// require.Equal(t, res.Status, abci.ResponseProcessProposal_ACCEPT)
		require.False(t, testWrapper.App.GetOptimisticProcessingInfo().Aborted)
	})
}
