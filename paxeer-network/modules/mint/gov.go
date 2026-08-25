package mint

import (
	"github.com/sidiora-labs/paxeer-network/modules/mint/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/mint/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

func HandleUpdateMinterProposal(ctx sdk.Context, k *keeper.Keeper, p *types.UpdateMinterProposal) error {
	err := types.ValidateMinter(*p.Minter)
	if err != nil {
		return err
	}
	k.SetMinter(ctx, *p.Minter)
	return nil
}
