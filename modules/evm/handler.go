package evm

import (
	"fmt"

	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
	govtypes "github.com/sidiora-labs/paxeer-network/sdk/x/gov/types"

	"github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
)

func NewHandler(k *keeper.Keeper) sdk.Handler {
	msgServer := keeper.NewMsgServerImpl(k)

	return func(ctx sdk.Context, msg sdk.Msg) (*sdk.Result, error) {
		ctx = ctx.WithEventManager(sdk.NewEventManager())

		switch msg := msg.(type) {
		case *types.MsgEVMTransaction:
			res, err := msgServer.EVMTransaction(sdk.WrapSDKContext(ctx), msg)
			return sdk.WrapServiceResult(ctx, res, err)
		case *types.MsgSend:
			res, err := msgServer.Send(sdk.WrapSDKContext(ctx), msg)
			return sdk.WrapServiceResult(ctx, res, err)
		case *types.MsgRegisterPointer:
			res, err := msgServer.RegisterPointer(sdk.WrapSDKContext(ctx), msg)
			return sdk.WrapServiceResult(ctx, res, err)
		case *types.MsgAssociateContractAddress:
			res, err := msgServer.AssociateContractAddress(sdk.WrapSDKContext(ctx), msg)
			return sdk.WrapServiceResult(ctx, res, err)
		case *types.MsgBindERCNativePointer:
			res, err := msgServer.BindERCNativePointer(sdk.WrapSDKContext(ctx), msg)
			return sdk.WrapServiceResult(ctx, res, err)
		default:
			errMsg := fmt.Sprintf("unrecognized %s message type: %T", types.ModuleName, msg)
			return nil, sdkerrors.Wrap(sdkerrors.ErrUnknownRequest, errMsg)
		}
	}
}

func NewProposalHandler(k keeper.Keeper) govtypes.Handler {
	msgServer := keeper.NewMsgServerImpl(&k)
	return func(ctx sdk.Context, content govtypes.Content) error {
		switch c := content.(type) {
		case *types.PointerBindingProposal:
			return HandlePointerBindingProposal(ctx, msgServer, c)
		case *types.AddERCNativePointerProposal:
			return HandleAddERCNativePointerProposal(ctx, &k, c)
		case *types.AddERCCW20PointerProposal:
			return HandleAddERCCW20PointerProposal(ctx, &k, c)
		case *types.AddERCCW721PointerProposal:
			return HandleAddERCCW721PointerProposal(ctx, &k, c)
		case *types.AddERCCW1155PointerProposal:
			return HandleAddERCCW1155PointerProposal(ctx, &k, c)
		case *types.AddCWERC20PointerProposal:
			return HandleAddCWERC20PointerProposal(ctx, &k, c)
		case *types.AddCWERC721PointerProposal:
			return HandleAddCWERC721PointerProposal(ctx, &k, c)
		case *types.AddCWERC1155PointerProposal:
			return HandleAddCWERC1155PointerProposal(ctx, &k, c)
		case *types.AddERCNativePointerProposalV2:
			return HandleAddERCNativePointerProposalV2(ctx, &k, c)
		default:
			return sdkerrors.Wrapf(sdkerrors.ErrUnknownRequest, "unrecognized evm proposal content type: %T", c)
		}
	}
}

// HandlePointerBindingProposal executes a passed PointerBindingProposal: every
// message it carries, in order, through the module's Msg service, which
// applies it only for the governance module account. A malformed proposal is
// refused before any message runs, and the messages execute together inside
// one cache context: if one fails, none of them changes the state.
func HandlePointerBindingProposal(ctx sdk.Context, msgServer types.MsgServer, p *types.PointerBindingProposal) error {
	if err := p.ValidateBasic(); err != nil {
		return err
	}
	msgs, err := p.GetMessages()
	if err != nil {
		return err
	}
	cacheCtx, write := ctx.CacheContext()
	for i, msg := range msgs {
		if err := executeGovernanceMessage(cacheCtx, msgServer, msg); err != nil {
			return sdkerrors.Wrapf(err, "message %d (%s)", i, sdk.MsgTypeURL(msg))
		}
	}
	write()
	ctx.EventManager().EmitEvents(cacheCtx.EventManager().Events())
	return nil
}

func executeGovernanceMessage(ctx sdk.Context, msgServer types.MsgServer, msg sdk.Msg) error {
	switch m := msg.(type) {
	case *types.MsgBindERCNativePointer:
		_, err := msgServer.BindERCNativePointer(sdk.WrapSDKContext(ctx), m)
		return err
	default:
		return sdkerrors.Wrapf(govtypes.ErrInvalidProposalContent, "%T is not a %s governance message", msg, types.ModuleName)
	}
}
