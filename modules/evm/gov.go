package evm

import (
	"errors"
	"fmt"
	"math"

	"github.com/ethereum/go-ethereum/core/vm"
	"github.com/paxeer-network/paxlog"
	"github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	govtypes "github.com/sidiora-labs/paxeer-network/sdk/x/gov/types"
	"github.com/sidiora-labs/paxeer-network/sdk/x/params"
	paramskeeper "github.com/sidiora-labs/paxeer-network/sdk/x/params/keeper"
	"github.com/sidiora-labs/paxeer-network/sdk/x/params/types/proposal"
	"github.com/sidiora-labs/paxeer-network/utils"
)

var logger = paxlog.NewLogger("x", "evm")

// NewParamChangeProposalHandler wraps the params proposal handler so a change to
// the x/evm allowed fee denoms reaches the store only when every updated rate is
// within max_fee_token_spread of the rate it replaces. Any other change passes
// through to the params handler unchanged.
func NewParamChangeProposalHandler(k *keeper.Keeper, paramsKeeper paramskeeper.Keeper) govtypes.Handler {
	next := params.NewParamChangeProposalHandler(paramsKeeper)
	return func(ctx sdk.Context, content govtypes.Content) error {
		p, ok := content.(*proposal.ParameterChangeProposal)
		if !ok || !changesAllowedFeeDenoms(p) {
			return next(ctx, content)
		}
		cacheCtx, write := ctx.CacheContext()
		if err := next(cacheCtx, content); err != nil {
			return err
		}
		if err := k.ValidateFeeTokenRateUpdate(ctx, k.GetAllowedFeeDenoms(cacheCtx)); err != nil {
			return err
		}
		write()
		return nil
	}
}

func changesAllowedFeeDenoms(p *proposal.ParameterChangeProposal) bool {
	for _, c := range p.Changes {
		if c.Subspace == types.ModuleName && c.Key == string(types.KeyAllowedFeeDenoms) {
			return true
		}
	}
	return false
}

func HandleAddERCNativePointerProposalV2(ctx sdk.Context, k *keeper.Keeper, p *types.AddERCNativePointerProposalV2) error {
	decimals := uint8(math.MaxUint8)
	if p.Decimals <= uint32(decimals) {
		// should always be the case given validation
		decimals = uint8(p.Decimals) //nolint:gosec
	}
	return k.RunWithOneOffEVMInstance(
		ctx, func(e *vm.EVM) error {
			_, err := k.UpsertERCNativePointer(ctx, e, p.Token, utils.ERCMetadata{Name: p.Name, Symbol: p.Symbol, Decimals: decimals})
			return err
		}, func(s1, s2 string) {
			logNativeV2Error(ctx, p, s1, s2)
		},
	)
}

func logNativeV2Error(ctx sdk.Context, p *types.AddERCNativePointerProposalV2, step string, err string) {
	id := fmt.Sprintf("Title: %s, Description: %s, Token: %s", p.Title, p.Description, p.Token)
	logger.Error("Proposal encountered error during step", "id", id, "step", step, "err", err)
}

func HandleAddERCNativePointerProposal(ctx sdk.Context, k *keeper.Keeper, p *types.AddERCNativePointerProposal) error {
	return errors.New("proposal type deprecated")
}

func HandleAddERCCW20PointerProposal(ctx sdk.Context, k *keeper.Keeper, p *types.AddERCCW20PointerProposal) error {
	return errors.New("proposal type deprecated")
}

func HandleAddERCCW721PointerProposal(ctx sdk.Context, k *keeper.Keeper, p *types.AddERCCW721PointerProposal) error {
	return errors.New("proposal type deprecated")
}

func HandleAddERCCW1155PointerProposal(ctx sdk.Context, k *keeper.Keeper, p *types.AddERCCW1155PointerProposal) error {
	return errors.New("proposal type deprecated")
}

func HandleAddCWERC20PointerProposal(ctx sdk.Context, k *keeper.Keeper, p *types.AddCWERC20PointerProposal) error {
	return errors.New("proposal type deprecated")
}

func HandleAddCWERC721PointerProposal(ctx sdk.Context, k *keeper.Keeper, p *types.AddCWERC721PointerProposal) error {
	return errors.New("proposal type deprecated")
}

func HandleAddCWERC1155PointerProposal(ctx sdk.Context, k *keeper.Keeper, p *types.AddCWERC1155PointerProposal) error {
	return errors.New("proposal type deprecated")
}
