package wasm

import (
	oraclekeeper "github.com/sidiora-labs/paxeer-network/modules/oracle/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/oracle/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

type OracleWasmQueryHandler struct {
	oracleKeeper oraclekeeper.Keeper
}

func NewOracleWasmQueryHandler(keeper *oraclekeeper.Keeper) *OracleWasmQueryHandler {
	return &OracleWasmQueryHandler{
		oracleKeeper: *keeper,
	}
}

func (handler OracleWasmQueryHandler) GetExchangeRates(ctx sdk.Context) (*types.QueryExchangeRatesResponse, error) {
	querier := oraclekeeper.NewQuerier(handler.oracleKeeper)
	c := sdk.WrapSDKContext(ctx)
	return querier.ExchangeRates(c, &types.QueryExchangeRatesRequest{})
}

func (handler OracleWasmQueryHandler) GetOracleTwaps(ctx sdk.Context, req *types.QueryTwapsRequest) (*types.QueryTwapsResponse, error) {
	querier := oraclekeeper.NewQuerier(handler.oracleKeeper)
	c := sdk.WrapSDKContext(ctx)
	return querier.Twaps(c, req)
}
