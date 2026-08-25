package wasm

import (
	"github.com/sidiora-labs/paxeer-network/modules/epoch/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/epoch/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

type EpochWasmQueryHandler struct {
	epochKeeper keeper.Keeper
}

func NewEpochWasmQueryHandler(keeper *keeper.Keeper) *EpochWasmQueryHandler {
	return &EpochWasmQueryHandler{
		epochKeeper: *keeper,
	}
}

func (handler EpochWasmQueryHandler) GetEpoch(ctx sdk.Context, req *types.QueryEpochRequest) (*types.QueryEpochResponse, error) {
	c := sdk.WrapSDKContext(ctx)
	return handler.epochKeeper.Epoch(c, req)
}
