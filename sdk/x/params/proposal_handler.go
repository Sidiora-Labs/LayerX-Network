package params

import (
	"github.com/paxeer-network/paxlog"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
	govtypes "github.com/sidiora-labs/paxeer-network/sdk/x/gov/types"
	"github.com/sidiora-labs/paxeer-network/sdk/x/params/keeper"
	"github.com/sidiora-labs/paxeer-network/sdk/x/params/types/proposal"
)

var logger = paxlog.NewLogger("cosmos", "x", "params")

// NewParamChangeProposalHandler creates a new governance Handler for a ParamChangeProposal
func NewParamChangeProposalHandler(k keeper.Keeper) govtypes.Handler {
	return func(ctx sdk.Context, content govtypes.Content) error {
		switch c := content.(type) {
		case *proposal.ParameterChangeProposal:
			return handleParameterChangeProposal(ctx, k, c)

		default:
			return sdkerrors.Wrapf(sdkerrors.ErrUnknownRequest, "unrecognized param proposal content type: %T", c)
		}
	}
}

func handleParameterChangeProposal(ctx sdk.Context, k keeper.Keeper, p *proposal.ParameterChangeProposal) error {
	for _, c := range p.Changes {
		ss, ok := k.GetSubspace(c.Subspace)
		if !ok {
			return sdkerrors.Wrap(proposal.ErrUnknownSubspace, c.Subspace)
		}

		logger.Info("attempt to set new parameter value", "key", c.Key, "value", c.Value)

		if err := ss.Update(ctx, []byte(c.Key), []byte(c.Value)); err != nil {
			return sdkerrors.Wrapf(proposal.ErrSettingParameter, "key: %s, value: %s, err: %s", c.Key, c.Value, err.Error())
		}
	}

	return nil
}
