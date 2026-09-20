package staking_test

import (
	"context"
	"testing"

	abci "github.com/sidiora-labs/paxeer-network/consensus/abci/types"
	tmproto "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/types"
	"github.com/stretchr/testify/require"

	paxapp "github.com/sidiora-labs/paxeer-network/node"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	authtypes "github.com/sidiora-labs/paxeer-network/sdk/x/auth/types"
	banktypes "github.com/sidiora-labs/paxeer-network/sdk/x/bank/types"
	"github.com/sidiora-labs/paxeer-network/sdk/x/staking/types"
)

func checkValidator(t *testing.T, app *paxapp.App, addr sdk.ValAddress, expFound bool) types.Validator {
	ctxCheck := app.BaseApp.NewContext(true, tmproto.Header{})
	validator, found := app.StakingKeeper.GetValidator(ctxCheck, addr)

	require.Equal(t, expFound, found)
	return validator
}

func checkDelegation(
	t *testing.T, app *paxapp.App, delegatorAddr sdk.AccAddress,
	validatorAddr sdk.ValAddress, expFound bool, expShares sdk.Dec,
) {

	ctxCheck := app.BaseApp.NewContext(true, tmproto.Header{})
	delegation, found := app.StakingKeeper.GetDelegation(ctxCheck, delegatorAddr, validatorAddr)
	if expFound {
		require.True(t, found)
		require.True(sdk.DecEq(t, expShares, delegation.Shares))

		return
	}

	require.False(t, found)
}

func TestStakingMsgs(t *testing.T) {
	genTokens := sdk.TokensFromConsensusPower(42, sdk.DefaultPowerReduction)
	bondTokens := sdk.TokensFromConsensusPower(10, sdk.DefaultPowerReduction)
	genCoin := sdk.NewCoin(sdk.DefaultBondDenom, genTokens)
	bondCoin := sdk.NewCoin(sdk.DefaultBondDenom, bondTokens)

	acc1 := &authtypes.BaseAccount{Address: addr1.String()}
	acc2 := &authtypes.BaseAccount{Address: addr2.String()}
	accs := authtypes.GenesisAccounts{acc1, acc2}
	balances := []banktypes.Balance{
		{
			Address: addr1.String(),
			Coins:   sdk.Coins{genCoin},
		},
		{
			Address: addr2.String(),
			Coins:   sdk.Coins{genCoin},
		},
	}

	app := paxapp.SetupWithGenesisAccounts(t, accs, balances...)
	paxapp.CheckBalance(t, app, addr1, sdk.Coins{genCoin})
	paxapp.CheckBalance(t, app, addr2, sdk.Coins{genCoin})

	// create validator
	description := types.NewDescription("foo_moniker", "", "", "", "")
	createValidatorMsg, err := types.NewMsgCreateValidator(
		sdk.ValAddress(addr1), valKey.PubKey(), bondCoin, description, commissionRates, sdk.OneInt(),
	)
	require.NoError(t, err)

	header := tmproto.Header{ChainID: app.ChainID, Height: app.LastBlockHeight() + 1}
	txGen := paxapp.MakeEncodingConfig().TxConfig
	_, _, err = paxapp.SignCheckDeliver(t, txGen, app.BaseApp, header, []sdk.Msg{createValidatorMsg}, []uint64{0}, []uint64{0}, true, true, priv1)
	require.NoError(t, err)
	paxapp.CheckBalance(t, app, addr1, sdk.Coins{genCoin.Sub(bondCoin)})

	header = tmproto.Header{ChainID: app.ChainID, Height: app.LastBlockHeight() + 1}
	app.FinalizeBlock(context.Background(), &abci.RequestFinalizeBlock{Header: &tmproto.Header{ChainID: app.ChainID, Height: app.LastBlockHeight() + 1}})

	validator := checkValidator(t, app, sdk.ValAddress(addr1), true)
	require.Equal(t, sdk.ValAddress(addr1).String(), validator.OperatorAddress)
	require.Equal(t, types.Bonded, validator.Status)
	require.True(sdk.IntEq(t, bondTokens, validator.BondedTokens()))

	header = tmproto.Header{ChainID: app.ChainID, Height: app.LastBlockHeight() + 1}
	app.FinalizeBlock(context.Background(), &abci.RequestFinalizeBlock{Header: &tmproto.Header{ChainID: app.ChainID, Height: app.LastBlockHeight() + 1}})

	// edit the validator
	description = types.NewDescription("bar_moniker", "", "", "", "")
	editValidatorMsg := types.NewMsgEditValidator(sdk.ValAddress(addr1), description, nil, nil)

	header = tmproto.Header{ChainID: app.ChainID, Height: app.LastBlockHeight() + 1}
	_, _, err = paxapp.SignCheckDeliver(t, txGen, app.BaseApp, header, []sdk.Msg{editValidatorMsg}, []uint64{0}, []uint64{1}, true, true, priv1)
	require.NoError(t, err)

	validator = checkValidator(t, app, sdk.ValAddress(addr1), true)
	require.Equal(t, description, validator.Description)

	// delegate
	paxapp.CheckBalance(t, app, addr2, sdk.Coins{genCoin})
	delegateMsg := types.NewMsgDelegate(addr2, sdk.ValAddress(addr1), bondCoin)

	header = tmproto.Header{ChainID: app.ChainID, Height: app.LastBlockHeight() + 1}
	_, _, err = paxapp.SignCheckDeliver(t, txGen, app.BaseApp, header, []sdk.Msg{delegateMsg}, []uint64{1}, []uint64{0}, true, true, priv2)
	require.NoError(t, err)

	paxapp.CheckBalance(t, app, addr2, sdk.Coins{genCoin.Sub(bondCoin)})
	checkDelegation(t, app, addr2, sdk.ValAddress(addr1), true, bondTokens.ToDec())

	// begin unbonding
	beginUnbondingMsg := types.NewMsgUndelegate(addr2, sdk.ValAddress(addr1), bondCoin)
	header = tmproto.Header{ChainID: app.ChainID, Height: app.LastBlockHeight() + 1}
	_, _, err = paxapp.SignCheckDeliver(t, txGen, app.BaseApp, header, []sdk.Msg{beginUnbondingMsg}, []uint64{1}, []uint64{1}, true, true, priv2)
	require.NoError(t, err)

	// delegation should exist anymore
	checkDelegation(t, app, addr2, sdk.ValAddress(addr1), false, sdk.Dec{})

	// balance should be the same because bonding not yet complete
	paxapp.CheckBalance(t, app, addr2, sdk.Coins{genCoin.Sub(bondCoin)})
}
