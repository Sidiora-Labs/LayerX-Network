package keeper

import (
	"errors"

	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/modules/layerxbridge/types"
	tokenfactorytypes "github.com/sidiora-labs/paxeer-network/modules/tokenfactory/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	banktypes "github.com/sidiora-labs/paxeer-network/sdk/x/bank/types"
)

func (k Keeper) EnsureSidioraDenom(ctx sdk.Context, chainID uint64) (string, error) {
	if _, found := k.GetChain(ctx, chainID); !found {
		return "", types.ErrUnknownChain.Wrapf("chain %d", chainID)
	}
	denom := types.SidioraDenom()
	creator, subdenom, err := tokenfactorytypes.DeconstructDenom(denom)
	if err != nil {
		return "", err
	}
	if creator != k.ModuleAddress().String() || subdenom != types.SidioraSubdenom {
		return "", types.ErrUnauthorized.Wrap("Sidiora creator must be the bridge module account")
	}
	record := types.BridgedAsset{
		ChainID: chainID,
		Asset:   types.Address20(common.HexToAddress(types.SidioraRemoteAddress)),
		Denom:   denom,
	}
	if existing, found := k.GetAsset(ctx, chainID, record.Asset); found && existing != record {
		return "", types.ErrInvalidRequest.Wrap("Sidiora remote asset already has another denom")
	}
	if existing, found := k.GetAssetByDenom(ctx, denom); found && existing != record {
		return "", types.ErrInvalidRequest.Wrap("Sidiora denom already has another remote asset")
	}
	cached, write := ctx.CacheContext()
	created, err := k.tokenFactory.CreateDenom(cached, creator, subdenom)
	if err != nil && !errors.Is(err, tokenfactorytypes.ErrDenomExists) {
		return "", err
	}
	if err == nil && created != denom {
		return "", types.ErrInvalidRequest.Wrapf("tokenfactory created %s instead of the Sidiora denom", created)
	}
	admin, err := k.tokenFactory.GetAuthorityMetadata(cached, denom)
	if err != nil {
		return "", err
	}
	if admin.Admin != creator {
		return "", types.ErrUnauthorized.Wrap("Sidiora admin must be the bridge module account")
	}
	_, err = k.tokenFactoryMsg.SetDenomMetadata(sdk.WrapSDKContext(cached), &tokenfactorytypes.MsgSetDenomMetadata{
		Sender: creator,
		Metadata: banktypes.Metadata{
			Description: "Sidiora, the second official coin of Paxeer X Network.",
			DenomUnits: []*banktypes.DenomUnit{
				{Denom: denom, Exponent: 0},
				{Denom: types.SidioraSymbol, Exponent: types.SidioraDecimals},
			},
			Base:    denom,
			Display: types.SidioraSymbol,
			Name:    "Sidiora",
			Symbol:  types.SidioraSymbol,
		},
	})
	if err != nil {
		return "", err
	}
	k.setAsset(cached, record)
	write()
	ctx.EventManager().EmitEvents(cached.EventManager().Events())
	return denom, nil
}
