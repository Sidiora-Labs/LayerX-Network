package keeper

import (
	"fmt"

	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/modules/xweb/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// Refund returns the fee of a request that was not fulfilled by its timeout
// height to its requester, exactly once. Anyone may trigger it; the fee only
// ever goes back to the requester. It refuses an unknown, fulfilled or
// already refunded request and one whose timeout height is not yet reached.
// A refunded request is never fulfilled. Refunds stay open while the module
// is paused so no fee is held by a pause.
func (k Keeper) Refund(ctx sdk.Context, id uint64) (types.Request, error) {
	request, found := k.GetRequest(ctx, id)
	if !found {
		return types.Request{}, types.ErrUnknownRequest.Wrapf("request %d", id)
	}
	switch request.Status {
	case types.StatusFulfilled:
		return types.Request{}, types.ErrAlreadyFulfilled.Wrapf("request %d", id)
	case types.StatusRefunded:
		return types.Request{}, types.ErrRefunded.Wrapf("request %d", id)
	}
	if ctx.BlockHeight() < request.TimeoutHeight {
		return types.Request{}, types.ErrNotExpired.Wrapf("request %d refundable at height %d, now %d",
			id, request.TimeoutHeight, ctx.BlockHeight())
	}

	cached, write := ctx.CacheContext()
	requester := k.evmKeeper.GetPaxAddressOrDefault(cached, common.Address(request.Requester))
	if err := k.bankKeeper.SendCoins(cached, k.ModuleAddress(), requester, k.feeCoins(request.Fee)); err != nil {
		return types.Request{}, err
	}
	request.Status = types.StatusRefunded
	k.setRequest(cached, request)
	write()
	ctx.EventManager().EmitEvents(cached.EventManager().Events())

	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventRefunded,
		sdk.NewAttribute(types.AttributeRequestID, fmt.Sprint(id)),
		sdk.NewAttribute(types.AttributeRequester, request.Requester.Hex()),
		sdk.NewAttribute(types.AttributeFee, request.Fee.String())))
	return request, nil
}
