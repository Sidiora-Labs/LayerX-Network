package keeper

import (
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
	"github.com/sidiora-labs/paxeer-network/sdk/x/distribution/types"
)

// HandleCommunityPoolSpendProposal is a handler for executing a passed community spend proposal
func HandleCommunityPoolSpendProposal(ctx sdk.Context, k Keeper, p *types.CommunityPoolSpendProposal) error {
	if k.blockedAddrs[p.Recipient] {
		return sdkerrors.Wrapf(sdkerrors.ErrUnauthorized, "%s is not allowed to receive external funds", p.Recipient)
	}

	recipient, err := sdk.AccAddressFromBech32(p.Recipient)
	if err != nil {
		return err
	}

	if err := k.DistributeFromFeePool(ctx, p.Amount, recipient); err != nil {
		return err
	}

	logger.Info("transferred from the community pool to recipient", "amount", p.Amount.String(), "recipient", p.Recipient)

	return nil
}
