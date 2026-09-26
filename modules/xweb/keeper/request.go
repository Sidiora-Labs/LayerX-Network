package keeper

import (
	"encoding/hex"
	"fmt"

	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/modules/xweb/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// Request stores a contract's request for web data under the next nonce and
// takes paid, which must equal the current fee exactly, from the requester's
// account into the module account. It refuses when paused, for an unknown
// kind, an empty payload or one over the payload cap, an api payload that
// does not decode or names an attestor outside the registered set, and a
// callback gas of zero or over the callback cap. It returns the request id.
func (k Keeper) Request(ctx sdk.Context, requester common.Address, kind uint8, payload []byte,
	callbackGas uint64, paid sdk.Int) (uint64, error) {
	if k.IsPaused(ctx) {
		return 0, types.ErrPaused
	}
	params := k.GetParams(ctx)
	if !types.KnownKind(kind) {
		return 0, types.ErrUnknownKind.Wrapf("kind %d", kind)
	}
	if len(payload) == 0 || uint64(len(payload)) > uint64(params.MaxPayloadBytes) {
		return 0, types.ErrPayloadSize.Wrapf("%d bytes, cap %d", len(payload), params.MaxPayloadBytes)
	}
	level, attestor := types.LevelMajority, types.Address20{}
	if kind == types.KindApi {
		api, err := types.DecodeApiPayload(payload)
		if err != nil {
			return 0, err
		}
		if err := api.CheckAttestors(k.GetAttestorSet(ctx)); err != nil {
			return 0, err
		}
		level, attestor = api.Level, api.Attestor
	}
	if callbackGas == 0 || callbackGas > params.MaxCallbackGas {
		return 0, types.ErrCallbackGas.Wrapf("%d, cap %d", callbackGas, params.MaxCallbackGas)
	}
	if paid.IsNil() || !paid.Equal(params.Fee) {
		return 0, types.ErrWrongFee.Wrapf("paid %s, fee %s", paid, params.Fee)
	}
	if requester == (common.Address{}) {
		return 0, types.ErrInvalidRequest.Wrap("zero requester")
	}

	cached, write := ctx.CacheContext()
	payer := k.evmKeeper.GetPaxAddressOrDefault(cached, requester)
	if err := k.bankKeeper.SendCoins(cached, payer, k.ModuleAddress(), k.feeCoins(params.Fee)); err != nil {
		return 0, err
	}
	id := k.Nonce(cached) + 1
	request := types.Request{
		ID:            id,
		Requester:     types.Address20(requester),
		Kind:          kind,
		PayloadHash:   types.Keccak(payload),
		CallbackGas:   callbackGas,
		Fee:           params.Fee,
		Height:        ctx.BlockHeight(),
		TimeoutHeight: ctx.BlockHeight() + int64(params.TimeoutBlocks),
		Status:        types.StatusPending,
		Level:         level,
		Attestor:      attestor,
	}
	k.setNonce(cached, id)
	k.setRequest(cached, request)
	write()
	ctx.EventManager().EmitEvents(cached.EventManager().Events())

	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventRequested,
		sdk.NewAttribute(types.AttributeRequestID, fmt.Sprint(id)),
		sdk.NewAttribute(types.AttributeOrigin, fmt.Sprint(types.OriginEVM)),
		sdk.NewAttribute(types.AttributeNetworkID, k.evmKeeper.ChainID(ctx).String()),
		sdk.NewAttribute(types.AttributeRequester, request.Requester.Hex()),
		sdk.NewAttribute(types.AttributeKind, fmt.Sprint(kind)),
		sdk.NewAttribute(types.AttributePayload, hex.EncodeToString(payload)),
		sdk.NewAttribute(types.AttributePayloadHash, request.PayloadHash.Hex()),
		sdk.NewAttribute(types.AttributeCallbackGas, fmt.Sprint(callbackGas)),
		sdk.NewAttribute(types.AttributeFee, params.Fee.String()),
		sdk.NewAttribute(types.AttributeHeight, fmt.Sprint(request.Height)),
		sdk.NewAttribute(types.AttributeTimeoutHeight, fmt.Sprint(request.TimeoutHeight)),
		sdk.NewAttribute(types.AttributeLevel, fmt.Sprint(level)),
		sdk.NewAttribute(types.AttributeAttestor, attestor.Hex())))
	return id, nil
}
