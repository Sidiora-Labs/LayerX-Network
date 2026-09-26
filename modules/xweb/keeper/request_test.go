package keeper_test

import (
	"encoding/hex"
	"fmt"
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/modules/xweb/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/stretchr/testify/require"
)

func TestRequestStoresUnderTheNextNonceAndTakesTheFee(t *testing.T) {
	s := newSuite(t, true)
	before := s.balance(s.account(requester))
	first := s.request()
	second, err := s.k.Request(s.ctx.WithBlockHeight(startHeight+3), requester, types.KindSearch, []byte("paxeer"),
		callbackGas, sdk.NewInt(fee))
	require.NoError(t, err)
	require.Equal(t, uint64(1), first)
	require.Equal(t, uint64(2), second)
	require.Equal(t, uint64(2), s.k.Nonce(s.ctx))

	require.Equal(t, before.Sub(sdk.NewInt(2*fee)), s.balance(s.account(requester)))
	require.Equal(t, sdk.NewInt(2*fee), s.balance(s.k.ModuleAddress()))

	request, found := s.k.GetRequest(s.ctx, first)
	require.True(t, found)
	require.Equal(t, types.Request{
		ID:            1,
		Requester:     types.Address20(requester),
		Kind:          types.KindFetch,
		PayloadHash:   types.Keccak(payload),
		CallbackGas:   callbackGas,
		Fee:           sdk.NewInt(fee),
		Height:        startHeight,
		TimeoutHeight: startHeight + int64(timeout),
		Status:        types.StatusPending,
	}, request)
	request, found = s.k.GetRequest(s.ctx, second)
	require.True(t, found)
	require.Equal(t, types.KindSearch, request.Kind)
	require.Equal(t, startHeight+3, request.Height)
	require.Equal(t, startHeight+3+int64(timeout), request.TimeoutHeight)
	_, found = s.k.GetResult(s.ctx, first)
	require.False(t, found)
}

func TestRequestEventCarriesWhatAnAttestorNeeds(t *testing.T) {
	s := newSuite(t, true)
	id := s.request()
	events := s.events(types.EventRequested)
	require.Len(t, events, 1)
	event := events[0]
	require.Equal(t, fmt.Sprint(id), attribute(event, types.AttributeRequestID))
	require.Equal(t, "1", attribute(event, types.AttributeOrigin))
	require.Equal(t, s.app.EvmKeeper.ChainID(s.ctx).String(), attribute(event, types.AttributeNetworkID))
	require.Equal(t, types.Address20(requester).Hex(), attribute(event, types.AttributeRequester))
	require.Equal(t, "1", attribute(event, types.AttributeKind))
	require.Equal(t, hex.EncodeToString(payload), attribute(event, types.AttributePayload))
	require.Equal(t, types.Keccak(payload).Hex(), attribute(event, types.AttributePayloadHash))
	require.Equal(t, fmt.Sprint(callbackGas), attribute(event, types.AttributeCallbackGas))
	require.Equal(t, fmt.Sprint(fee), attribute(event, types.AttributeFee))
	require.Equal(t, fmt.Sprint(startHeight), attribute(event, types.AttributeHeight))
	require.Equal(t, fmt.Sprint(startHeight+int64(timeout)), attribute(event, types.AttributeTimeoutHeight))
}

func TestRequestRefusals(t *testing.T) {
	s := newSuite(t, true)
	before := s.balance(s.account(requester))
	for name, tc := range map[string]struct {
		kind    uint8
		payload []byte
		gas     uint64
		paid    sdk.Int
		err     error
	}{
		"unknown kind":      {0, payload, callbackGas, sdk.NewInt(fee), types.ErrUnknownKind},
		"kind three":        {3, payload, callbackGas, sdk.NewInt(fee), types.ErrUnknownKind},
		"empty payload":     {types.KindFetch, nil, callbackGas, sdk.NewInt(fee), types.ErrPayloadSize},
		"payload over cap":  {types.KindFetch, make([]byte, payloadCap+1), callbackGas, sdk.NewInt(fee), types.ErrPayloadSize},
		"zero callback gas": {types.KindFetch, payload, 0, sdk.NewInt(fee), types.ErrCallbackGas},
		"callback over cap": {types.KindFetch, payload, callbackCap + 1, sdk.NewInt(fee), types.ErrCallbackGas},
		"fee too low":       {types.KindFetch, payload, callbackGas, sdk.NewInt(fee - 1), types.ErrWrongFee},
		"fee too high":      {types.KindFetch, payload, callbackGas, sdk.NewInt(fee + 1), types.ErrWrongFee},
		"no fee":            {types.KindFetch, payload, callbackGas, sdk.ZeroInt(), types.ErrWrongFee},
		"nil fee":           {types.KindFetch, payload, callbackGas, sdk.Int{}, types.ErrWrongFee},
	} {
		_, err := s.k.Request(s.ctx, requester, tc.kind, tc.payload, tc.gas, tc.paid)
		require.ErrorIs(t, err, tc.err, name)
	}
	_, err := s.k.Request(s.ctx, requester, types.KindFetch, make([]byte, payloadCap), callbackCap, sdk.NewInt(fee))
	require.NoError(t, err, "payload and callback gas exactly at their caps")

	_, err = s.k.Request(s.ctx, common.Address{}, types.KindFetch, payload, callbackGas, sdk.NewInt(fee))
	require.ErrorIs(t, err, types.ErrInvalidRequest)

	poor := common.HexToAddress("0x00000000000000000000000000000000000b0b00")
	_, err = s.k.Request(s.ctx, poor, types.KindFetch, payload, callbackGas, sdk.NewInt(fee))
	require.Error(t, err, "a requester without the fee is refused")
	require.Equal(t, uint64(1), s.k.Nonce(s.ctx), "a refused request takes no nonce")
	require.Equal(t, before.Sub(sdk.NewInt(fee)), s.balance(s.account(requester)))

	require.NoError(t, s.k.Pause(s.ctx, types.MsgPause{Authority: authority}))
	_, err = s.k.Request(s.ctx, requester, types.KindFetch, payload, callbackGas, sdk.NewInt(fee))
	require.ErrorIs(t, err, types.ErrPaused)
}
