package wasm

import (
	tokenfactorykeeper "github.com/sidiora-labs/paxeer-network/modules/tokenfactory/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/tokenfactory/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

type TokenFactoryWasmQueryHandler struct {
	tokenfactoryKeeper tokenfactorykeeper.Keeper
}

func NewTokenFactoryWasmQueryHandler(keeper *tokenfactorykeeper.Keeper) *TokenFactoryWasmQueryHandler {
	return &TokenFactoryWasmQueryHandler{
		tokenfactoryKeeper: *keeper,
	}
}

func (handler TokenFactoryWasmQueryHandler) GetDenomAuthorityMetadata(ctx sdk.Context, req *types.QueryDenomAuthorityMetadataRequest) (*types.QueryDenomAuthorityMetadataResponse, error) {
	c := sdk.WrapSDKContext(ctx)
	return handler.tokenfactoryKeeper.DenomAuthorityMetadata(c, req)
}

func (handler TokenFactoryWasmQueryHandler) GetDenomsFromCreator(ctx sdk.Context, req *types.QueryDenomsFromCreatorRequest) (*types.QueryDenomsFromCreatorResponse, error) {
	denoms := handler.tokenfactoryKeeper.GetAllDenomsFromCreator(ctx, req.Creator)
	return &types.QueryDenomsFromCreatorResponse{Denoms: denoms}, nil
}
