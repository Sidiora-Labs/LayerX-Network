package slashing_test

import (
	"context"
	"errors"
	"testing"

	abci "github.com/sidiora-labs/paxeer-network/consensus/abci/types"
	tmproto "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/types"
	"github.com/stretchr/testify/require"

	paxapp "github.com/sidiora-labs/paxeer-network/node"
	"github.com/sidiora-labs/paxeer-network/sdk/crypto/keys/ed25519"
	"github.com/sidiora-labs/paxeer-network/sdk/crypto/keys/secp256k1"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	authtypes "github.com/sidiora-labs/paxeer-network/sdk/x/auth/types"
	banktypes "github.com/sidiora-labs/paxeer-network/sdk/x/bank/types"
	"github.com/sidiora-labs/paxeer-network/sdk/x/slashing/types"
	stakingtypes "github.com/sidiora-labs/paxeer-network/sdk/x/staking/types"
)

var (
	priv1 = secp256k1.GenPrivKey()
	addr1 = sdk.AccAddress(priv1.PubKey().Address())

	valKey  = ed25519.GenPrivKey()
	valAddr = sdk.AccAddress(valKey.PubKey().Address())
)

func checkValidator(t *testing.T, app *paxapp.App, _ sdk.AccAddress, expFound bool) stakingtypes.Validator {
	ctxCheck := app.BaseApp.NewContext(true, tmproto.Header{})
	validator, found := app.StakingKeeper.GetValidator(ctxCheck, sdk.ValAddress(addr1))
	require.Equal(t, expFound, found)
	return validator
}

func checkValidatorSigningInfo(t *testing.T, app *paxapp.App, addr sdk.ConsAddress, expFound bool) types.ValidatorSigningInfo {
	ctxCheck := app.BaseApp.NewContext(true, tmproto.Header{})
	signingInfo, found := app.SlashingKeeper.GetValidatorSigningInfo(ctxCheck, addr)
	require.Equal(t, expFound, found)
	return signingInfo
}

func TestSlashingMsgs(t *testing.T) {
	genTokens := sdk.TokensFromConsensusPower(42, sdk.DefaultPowerReduction)
	bondTokens := sdk.TokensFromConsensusPower(10, sdk.DefaultPowerReduction)
	genCoin := sdk.NewCoin(sdk.DefaultBondDenom, genTokens)
	bondCoin := sdk.NewCoin(sdk.DefaultBondDenom, bondTokens)

	acc1 := &authtypes.BaseAccount{
		Address: addr1.String(),
	}
	accs := authtypes.GenesisAccounts{acc1}
	balances := []banktypes.Balance{
		{
			Address: addr1.String(),
			Coins:   sdk.Coins{genCoin},
		},
	}

	app := paxapp.SetupWithGenesisAccounts(t, accs, balances...)
	paxapp.CheckBalance(t, app, addr1, sdk.Coins{genCoin})

	description := stakingtypes.NewDescription("foo_moniker", "", "", "", "")
	commission := stakingtypes.NewCommissionRates(sdk.NewDecWithPrec(5, 2), sdk.NewDecWithPrec(5, 2), sdk.ZeroDec())

	createValidatorMsg, err := stakingtypes.NewMsgCreateValidator(
		sdk.ValAddress(addr1), valKey.PubKey(), bondCoin, description, commission, sdk.OneInt(),
	)
	require.NoError(t, err)

	header := tmproto.Header{ChainID: app.ChainID, Height: app.LastBlockHeight() + 1}
	txGen := paxapp.MakeEncodingConfig().TxConfig
	_, _, err = paxapp.SignCheckDeliver(t, txGen, app.BaseApp, header, []sdk.Msg{createValidatorMsg}, []uint64{0}, []uint64{0}, true, true, priv1)
	require.NoError(t, err)
	paxapp.CheckBalance(t, app, addr1, sdk.Coins{genCoin.Sub(bondCoin)})

	app.FinalizeBlock(context.Background(), &abci.RequestFinalizeBlock{Header: &tmproto.Header{ChainID: app.ChainID, Height: app.LastBlockHeight() + 1}})

	validator := checkValidator(t, app, addr1, true)
	require.Equal(t, sdk.ValAddress(addr1).String(), validator.OperatorAddress)
	require.Equal(t, stakingtypes.Bonded, validator.Status)
	require.True(sdk.IntEq(t, bondTokens, validator.BondedTokens()))
	unjailMsg := &types.MsgUnjail{ValidatorAddr: sdk.ValAddress(addr1).String()}

	checkValidatorSigningInfo(t, app, sdk.ConsAddress(valAddr), true)

	// unjail should fail with unknown validator
	header = tmproto.Header{ChainID: app.ChainID, Height: app.LastBlockHeight() + 1}
	_, res, err := paxapp.SignCheckDeliver(t, txGen, app.BaseApp, header, []sdk.Msg{unjailMsg}, []uint64{0}, []uint64{1}, false, false, priv1)
	require.Error(t, err)
	require.Nil(t, res)
	require.True(t, errors.Is(err, types.ErrValidatorNotJailed))
}
