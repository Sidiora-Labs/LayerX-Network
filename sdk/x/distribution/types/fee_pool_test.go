package types_test

import (
	"testing"

	"github.com/stretchr/testify/require"

	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/sdk/x/distribution/types"
)

func TestValidateGenesis(t *testing.T) {

	fp := types.InitialFeePool()
	require.Nil(t, fp.ValidateGenesis())

	fp2 := types.FeePool{CommunityPool: sdk.DecCoins{{Denom: "uhpx", Amount: sdk.NewDec(-1)}}}
	require.NotNil(t, fp2.ValidateGenesis())
}
