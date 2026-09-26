package layerxbridge

import (
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
	govtypes "github.com/sidiora-labs/paxeer-network/sdk/x/gov/types"

	"github.com/sidiora-labs/paxeer-network/modules/layerxbridge/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/layerxbridge/types"
)

// NewProposalHandler executes a passed BridgeProposal: every message it
// carries, in order, through the module's Msg service and so through the
// keeper, which applies it only for the module's authority. The governance
// module account is the authority a proposal executes with, so a proposal
// carrying a message for any other authority, or a malformed proposal, is
// refused before any message runs. The messages execute together: if one
// fails, none of them changes the state.
func NewProposalHandler(k keeper.Keeper) govtypes.Handler {
	msgServer := keeper.NewMsgServerImpl(k)
	return func(ctx sdk.Context, content govtypes.Content) error {
		proposal, ok := content.(*types.BridgeProposal)
		if !ok {
			return sdkerrors.Wrapf(sdkerrors.ErrUnknownRequest, "unrecognized %s proposal content type: %T", types.ModuleName, content)
		}
		if err := proposal.ValidateBasic(); err != nil {
			return err
		}
		msgs, err := proposal.GetMessages()
		if err != nil {
			return err
		}
		cacheCtx, write := ctx.CacheContext()
		for i, msg := range msgs {
			if err := execute(cacheCtx, msgServer, msg); err != nil {
				return sdkerrors.Wrapf(err, "message %d (%s)", i, sdk.MsgTypeURL(msg))
			}
		}
		write()
		ctx.EventManager().EmitEvents(cacheCtx.EventManager().Events())
		return nil
	}
}

func execute(ctx sdk.Context, msgServer types.MsgServer, msg sdk.Msg) error {
	goCtx := sdk.WrapSDKContext(ctx)
	var err error
	switch m := msg.(type) {
	case *types.MsgRegisterChain:
		_, err = msgServer.RegisterChain(goCtx, m)
	case *types.MsgSetAttestors:
		_, err = msgServer.SetAttestors(goCtx, m)
	case *types.MsgSetCap:
		_, err = msgServer.SetCap(goCtx, m)
	case *types.MsgPause:
		_, err = msgServer.Pause(goCtx, m)
	case *types.MsgUnpause:
		_, err = msgServer.Unpause(goCtx, m)
	default:
		err = sdkerrors.Wrapf(govtypes.ErrInvalidProposalContent, "%T is not a %s governance message", msg, types.ModuleName)
	}
	return err
}
