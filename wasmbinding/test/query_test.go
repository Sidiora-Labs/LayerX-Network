package wasmbinding

import (
	"encoding/json"
	"fmt"
	"testing"
	"time"

	epochwasm "github.com/sidiora-labs/paxeer-network/modules/epoch/client/wasm"
	epochbinding "github.com/sidiora-labs/paxeer-network/modules/epoch/client/wasm/bindings"
	epochtypes "github.com/sidiora-labs/paxeer-network/modules/epoch/types"
	evmwasm "github.com/sidiora-labs/paxeer-network/modules/evm/client/wasm"
	oraclewasm "github.com/sidiora-labs/paxeer-network/modules/oracle/client/wasm"
	oraclebinding "github.com/sidiora-labs/paxeer-network/modules/oracle/client/wasm/bindings"
	oracletypes "github.com/sidiora-labs/paxeer-network/modules/oracle/types"
	oracleutils "github.com/sidiora-labs/paxeer-network/modules/oracle/utils"
	tokenfactorywasm "github.com/sidiora-labs/paxeer-network/modules/tokenfactory/client/wasm"
	tokenfactorybinding "github.com/sidiora-labs/paxeer-network/modules/tokenfactory/client/wasm/bindings"
	tokenfactorytypes "github.com/sidiora-labs/paxeer-network/modules/tokenfactory/types"
	"github.com/sidiora-labs/paxeer-network/node"
	"github.com/sidiora-labs/paxeer-network/sdk/crypto/keys/secp256k1"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	wasmvmtypes "github.com/sidiora-labs/paxeer-network/wasm-runtime/types"
	wasmkeeper "github.com/sidiora-labs/paxeer-network/wasm/x/wasm/keeper"
	"github.com/sidiora-labs/paxeer-network/wasmbinding"
	"github.com/stretchr/testify/require"
)

func SetupWasmbindingTest(t *testing.T) (*app.TestWrapper, func(ctx sdk.Context, request json.RawMessage) ([]byte, error)) {
	tm := time.Now().UTC()
	valPub := secp256k1.GenPrivKey().PubKey()

	testWrapper := app.NewTestWrapper(t, tm, valPub, false)

	oh := oraclewasm.NewOracleWasmQueryHandler(&testWrapper.App.OracleKeeper)
	eh := epochwasm.NewEpochWasmQueryHandler(&testWrapper.App.EpochKeeper)
	th := tokenfactorywasm.NewTokenFactoryWasmQueryHandler(&testWrapper.App.TokenFactoryKeeper)
	evmh := evmwasm.NewEVMQueryHandler(&testWrapper.App.EvmKeeper)
	qp := wasmbinding.NewQueryPlugin(oh, eh, th, evmh, testWrapper.App.StakingKeeper)
	return testWrapper, wasmbinding.CustomQuerier(qp)
}

func TestWasmUnknownQuery(t *testing.T) {
	testWrapper, customQuerier := SetupWasmbindingTest(t)

	oracle_req := oraclebinding.PaxOracleQuery{}
	queryData, err := json.Marshal(oracle_req)
	require.NoError(t, err)
	query := wasmbinding.PaxQueryWrapper{Route: wasmbinding.OracleRoute, QueryData: queryData}
	rawQuery, err := json.Marshal(query)
	require.NoError(t, err)

	_, err = customQuerier(testWrapper.Ctx, rawQuery)
	require.Error(t, err)
	require.Equal(t, err, oracletypes.ErrUnknownPaxOracleQuery)

	epoch_req := epochbinding.PaxEpochQuery{}
	queryData, err = json.Marshal(epoch_req)
	require.NoError(t, err)
	query = wasmbinding.PaxQueryWrapper{Route: wasmbinding.EpochRoute, QueryData: queryData}
	rawQuery, err = json.Marshal(query)
	require.NoError(t, err)

	_, err = customQuerier(testWrapper.Ctx, rawQuery)
	require.Error(t, err)
	require.Equal(t, err, epochtypes.ErrUnknownPaxEpochQuery)
}

func TestWasmGetOracleExchangeRates(t *testing.T) {
	testWrapper, customQuerier := SetupWasmbindingTest(t)

	req := oraclebinding.PaxOracleQuery{ExchangeRates: &oracletypes.QueryExchangeRatesRequest{}}
	queryData, err := json.Marshal(req)
	require.NoError(t, err)
	query := wasmbinding.PaxQueryWrapper{Route: wasmbinding.OracleRoute, QueryData: queryData}

	rawQuery, err := json.Marshal(query)
	require.NoError(t, err)

	res, err := customQuerier(testWrapper.Ctx, rawQuery)
	require.NoError(t, err)

	var parsedRes oracletypes.QueryExchangeRatesResponse
	err = json.Unmarshal(res, &parsedRes)
	require.NoError(t, err)
	require.Equal(t, oracletypes.QueryExchangeRatesResponse{DenomOracleExchangeRatePairs: oracletypes.DenomOracleExchangeRatePairs{}}, parsedRes)

	testWrapper.Ctx = testWrapper.Ctx.WithBlockHeight(11)
	testWrapper.App.OracleKeeper.SetBaseExchangeRate(testWrapper.Ctx, oracleutils.MicroAtomDenom, sdk.NewDec(12))

	res, err = customQuerier(testWrapper.Ctx, rawQuery)
	require.NoError(t, err)

	var parsedRes2 oracletypes.QueryExchangeRatesResponse
	err = json.Unmarshal(res, &parsedRes2)
	require.NoError(t, err)
	require.Equal(t, oracletypes.QueryExchangeRatesResponse{DenomOracleExchangeRatePairs: oracletypes.DenomOracleExchangeRatePairs{oracletypes.NewDenomOracleExchangeRatePair(oracleutils.MicroAtomDenom, sdk.NewDec(12), sdk.NewInt(11), testWrapper.Ctx.BlockTime().UnixMilli())}}, parsedRes2)
}

func TestWasmGetOracleTwaps(t *testing.T) {
	testWrapper, customQuerier := SetupWasmbindingTest(t)

	req := oraclebinding.PaxOracleQuery{OracleTwaps: &oracletypes.QueryTwapsRequest{LookbackSeconds: 200}}
	queryData, err := json.Marshal(req)
	require.NoError(t, err)
	query := wasmbinding.PaxQueryWrapper{Route: wasmbinding.OracleRoute, QueryData: queryData}

	rawQuery, err := json.Marshal(query)
	require.NoError(t, err)

	// this should error because there is no snapshots to build twap from
	_, err = customQuerier(testWrapper.Ctx, rawQuery)
	require.Error(t, err)

	testWrapper.Ctx = testWrapper.Ctx.WithBlockHeight(11).WithBlockTime(time.Unix(3600, 0))
	testWrapper.App.OracleKeeper.SetBaseExchangeRate(testWrapper.Ctx, oracleutils.MicroAtomDenom, sdk.NewDec(12))

	priceSnapshot := oracletypes.PriceSnapshot{SnapshotTimestamp: 3600, PriceSnapshotItems: oracletypes.PriceSnapshotItems{
		oracletypes.NewPriceSnapshotItem(oracleutils.MicroAtomDenom, oracletypes.OracleExchangeRate{ExchangeRate: sdk.NewDec(20), LastUpdate: sdk.NewInt(10)}),
	}}
	testWrapper.App.OracleKeeper.AddPriceSnapshot(testWrapper.Ctx, priceSnapshot)
	testWrapper.App.OracleKeeper.SetVoteTarget(testWrapper.Ctx, oracleutils.MicroAtomDenom)

	testWrapper.Ctx = testWrapper.Ctx.WithBlockHeight(14).WithBlockTime(time.Unix(3700, 0))

	res, err := customQuerier(testWrapper.Ctx, rawQuery)
	require.NoError(t, err)

	var parsedRes2 oracletypes.QueryTwapsResponse
	err = json.Unmarshal(res, &parsedRes2)
	require.NoError(t, err)
	// should be 100 isntead of 200 because thats the oldest data timestamp we have
	require.Equal(t, oracletypes.QueryTwapsResponse{OracleTwaps: oracletypes.OracleTwaps{
		oracletypes.OracleTwap{Denom: oracleutils.MicroAtomDenom, Twap: sdk.NewDec(20), LookbackSeconds: 100},
	}}, parsedRes2)
}

func TestWasmGetOracleTwapsErrorHandling(t *testing.T) {
	testWrapper, customQuerier := SetupWasmbindingTest(t)

	req := oraclebinding.PaxOracleQuery{OracleTwaps: &oracletypes.QueryTwapsRequest{LookbackSeconds: 200}}
	queryData, err := json.Marshal(req)
	require.NoError(t, err)
	query := wasmbinding.PaxQueryWrapper{Route: wasmbinding.OracleRoute, QueryData: queryData}
	rawQuery, err := json.Marshal(query)
	require.NoError(t, err)

	_, err = customQuerier(testWrapper.Ctx, rawQuery)
	require.Error(t, err)
	require.Equal(t, err, oracletypes.ErrNoTwapData)

	req = oraclebinding.PaxOracleQuery{OracleTwaps: &oracletypes.QueryTwapsRequest{LookbackSeconds: 3601}}
	queryData, err = json.Marshal(req)
	require.NoError(t, err)
	query = wasmbinding.PaxQueryWrapper{Route: wasmbinding.OracleRoute, QueryData: queryData}
	rawQuery, err = json.Marshal(query)
	require.NoError(t, err)

	_, err = customQuerier(testWrapper.Ctx, rawQuery)
	require.Error(t, err)
	require.Equal(t, err, oracletypes.ErrInvalidTwapLookback)
}

func TestWasmGetEpoch(t *testing.T) {
	testWrapper, customQuerier := SetupWasmbindingTest(t)

	req := epochbinding.PaxEpochQuery{
		Epoch: &epochtypes.QueryEpochRequest{},
	}

	queryData, err := json.Marshal(req)
	require.NoError(t, err)
	query := wasmbinding.PaxQueryWrapper{Route: wasmbinding.EpochRoute, QueryData: queryData}

	rawQuery, err := json.Marshal(query)
	require.NoError(t, err)

	testWrapper.Ctx = testWrapper.Ctx.WithBlockHeight(45).WithBlockTime(time.Unix(12500, 0))
	testWrapper.App.EpochKeeper.SetEpoch(testWrapper.Ctx, epochtypes.Epoch{
		GenesisTime:           time.Unix(1000, 0).UTC(),
		EpochDuration:         time.Minute,
		CurrentEpoch:          uint64(69),
		CurrentEpochStartTime: time.Unix(12345, 0).UTC(),
		CurrentEpochHeight:    int64(40),
	})

	res, err := customQuerier(testWrapper.Ctx, rawQuery)
	require.NoError(t, err)

	var parsedRes epochtypes.QueryEpochResponse
	err = json.Unmarshal(res, &parsedRes)
	require.NoError(t, err)
	epoch := parsedRes.Epoch
	require.Equal(t, time.Unix(1000, 0).UTC(), epoch.GenesisTime)
	require.Equal(t, time.Minute, epoch.EpochDuration)
	require.Equal(t, uint64(69), epoch.CurrentEpoch)
	require.Equal(t, time.Unix(12345, 0).UTC(), epoch.CurrentEpochStartTime)
	require.Equal(t, int64(40), epoch.CurrentEpochHeight)
}

func TestWasmGetDenomAuthorityMetadata(t *testing.T) {
	testWrapper, customQuerier := SetupWasmbindingTest(t)

	denom := fmt.Sprintf("factory/%s/test", app.TestUser)
	testWrapper.Ctx = testWrapper.Ctx.WithBlockHeight(11).WithBlockTime(time.Unix(3600, 0))
	// Create denom
	testWrapper.App.TokenFactoryKeeper.CreateDenom(testWrapper.Ctx, app.TestUser, "test")
	authorityMetadata := tokenfactorytypes.DenomAuthorityMetadata{
		Admin: app.TestUser,
	}

	// Setup tfk query
	req := tokenfactorybinding.PaxTokenFactoryQuery{DenomAuthorityMetadata: &tokenfactorytypes.QueryDenomAuthorityMetadataRequest{Denom: denom}}
	queryData, err := json.Marshal(req)
	require.NoError(t, err)
	query := wasmbinding.PaxQueryWrapper{Route: wasmbinding.TokenFactoryRoute, QueryData: queryData}

	rawQuery, err := json.Marshal(query)
	require.NoError(t, err)

	res, err := customQuerier(testWrapper.Ctx, rawQuery)
	require.NoError(t, err)

	var parsedRes tokenfactorytypes.QueryDenomAuthorityMetadataResponse
	err = json.Unmarshal(res, &parsedRes)
	require.NoError(t, err)
	require.Equal(t, tokenfactorytypes.QueryDenomAuthorityMetadataResponse{AuthorityMetadata: authorityMetadata}, parsedRes)
}

func TestWasmGetDenomsFromCreator(t *testing.T) {
	testWrapper, customQuerier := SetupWasmbindingTest(t)

	denom1 := fmt.Sprintf("factory/%s/test1", app.TestUser)
	denom2 := fmt.Sprintf("factory/%s/test2", app.TestUser)
	testWrapper.Ctx = testWrapper.Ctx.WithBlockHeight(11).WithBlockTime(time.Unix(3600, 0))

	// No denoms created initially
	req := tokenfactorybinding.PaxTokenFactoryQuery{DenomsFromCreator: &tokenfactorytypes.QueryDenomsFromCreatorRequest{Creator: app.TestUser}}
	queryData, err := json.Marshal(req)
	require.NoError(t, err)
	query := wasmbinding.PaxQueryWrapper{Route: wasmbinding.TokenFactoryRoute, QueryData: queryData}

	rawQuery, err := json.Marshal(query)
	require.NoError(t, err)

	res, err := customQuerier(testWrapper.Ctx, rawQuery)
	require.NoError(t, err)

	var parsedRes tokenfactorytypes.QueryDenomsFromCreatorResponse
	err = json.Unmarshal(res, &parsedRes)
	require.NoError(t, err)
	require.Empty(t, parsedRes.Denoms)

	// Add first denom
	testWrapper.App.TokenFactoryKeeper.CreateDenom(testWrapper.Ctx, app.TestUser, "test1")

	res, err = customQuerier(testWrapper.Ctx, rawQuery)
	require.NoError(t, err)

	var parsedRes2 tokenfactorytypes.QueryDenomsFromCreatorResponse
	err = json.Unmarshal(res, &parsedRes2)
	require.NoError(t, err)
	require.Equal(t, []string{denom1}, parsedRes2.Denoms)

	// Add second denom
	testWrapper.App.TokenFactoryKeeper.CreateDenom(testWrapper.Ctx, app.TestUser, "test2")

	res, err = customQuerier(testWrapper.Ctx, rawQuery)
	require.NoError(t, err)

	var parsedRes3 tokenfactorytypes.QueryDenomsFromCreatorResponse
	err = json.Unmarshal(res, &parsedRes3)
	require.NoError(t, err)
	require.Equal(t, []string{denom1, denom2}, parsedRes3.Denoms)

}

func MockQueryPlugins() wasmkeeper.QueryPlugins {
	return wasmkeeper.QueryPlugins{
		Bank: func(ctx sdk.Context, request *wasmvmtypes.BankQuery) ([]byte, error) { return []byte{}, nil },
		IBC: func(ctx sdk.Context, caller sdk.AccAddress, request *wasmvmtypes.IBCQuery) ([]byte, error) {
			return []byte{}, nil
		},
		Custom: func(ctx sdk.Context, request json.RawMessage) ([]byte, error) {
			return []byte{}, nil
		},
		Stargate: func(ctx sdk.Context, request *wasmvmtypes.StargateQuery) ([]byte, error) { return []byte{}, nil },
		Staking:  func(ctx sdk.Context, request *wasmvmtypes.StakingQuery) ([]byte, error) { return []byte{}, nil },
		Wasm:     func(ctx sdk.Context, request *wasmvmtypes.WasmQuery) ([]byte, error) { return []byte{}, nil },
	}
}
