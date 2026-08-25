//go:build norace
// +build norace

package testutil

import (
	"testing"
	"time"

	"github.com/sidiora-labs/paxeer-network/sdk/testutil/network"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/sdk/x/gov/types"

	"github.com/stretchr/testify/require"
	"github.com/stretchr/testify/suite"
)

func TestIntegrationTestSuite(t *testing.T) {
	cfg := network.DefaultConfig()
	cfg.NumValidators = 1
	suite.Run(t, NewIntegrationTestSuite(cfg))

	genesisState := types.DefaultGenesisState()
	genesisState.DepositParams = types.NewDepositParams(
		sdk.NewCoins(sdk.NewCoin(cfg.BondDenom, types.DefaultMinDepositTokens)),
		sdk.NewCoins(sdk.NewCoin(cfg.BondDenom, types.DefaultMinExpeditedDepositTokens)),
		time.Duration(15)*time.Second)
	genesisState.VotingParams = types.NewVotingParams(time.Duration(5)*time.Second, time.Duration(2)*time.Second)
	bz, err := cfg.Codec.MarshalAsJSON(genesisState)
	require.NoError(t, err)
	cfg.GenesisState["gov"] = bz
	suite.Run(t, NewDepositTestSuite(cfg))
}
