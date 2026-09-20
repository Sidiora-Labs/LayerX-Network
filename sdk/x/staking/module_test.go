package staking_test

import (
	"context"
	"testing"

	abcitypes "github.com/sidiora-labs/paxeer-network/consensus/abci/types"
	tmproto "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/types"
	"github.com/stretchr/testify/require"

	paxapp "github.com/sidiora-labs/paxeer-network/node"
	authtypes "github.com/sidiora-labs/paxeer-network/sdk/x/auth/types"
	"github.com/sidiora-labs/paxeer-network/sdk/x/staking/types"
)

func TestItCreatesModuleAccountOnInitBlock(t *testing.T) {
	app := paxapp.Setup(t, false, false, false)
	ctx := app.BaseApp.NewContext(false, tmproto.Header{})

	app.InitChain(
		context.Background(), &abcitypes.RequestInitChain{
			AppStateBytes: []byte("{}"),
			ChainId:       "test-chain-id",
		},
	)

	acc := app.AccountKeeper.GetAccount(ctx, authtypes.NewModuleAddress(types.BondedPoolName))
	require.NotNil(t, acc)

	acc = app.AccountKeeper.GetAccount(ctx, authtypes.NewModuleAddress(types.NotBondedPoolName))
	require.NotNil(t, acc)
}
