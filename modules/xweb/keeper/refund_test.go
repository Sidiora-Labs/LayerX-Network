package keeper_test

import (
	"fmt"
	"testing"

	"github.com/sidiora-labs/paxeer-network/modules/xweb/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/stretchr/testify/require"
)

func TestRefundAfterTimeoutReturnsTheFeeOnce(t *testing.T) {
	s := newSuite(t, true)
	before := s.balance(s.account(requester))
	id := s.request()
	timeoutHeight := startHeight + int64(timeout)

	_, err := s.k.Refund(s.ctx.WithBlockHeight(timeoutHeight-1), id)
	require.ErrorIs(t, err, types.ErrNotExpired)

	later := s.ctx.WithBlockHeight(timeoutHeight)
	request, err := s.k.Refund(later, id)
	require.NoError(t, err)
	require.Equal(t, types.StatusRefunded, request.Status)
	require.Equal(t, before, s.balance(s.account(requester)))
	require.True(t, s.balance(s.k.ModuleAddress()).IsZero())
	stored, _ := s.k.GetRequest(s.ctx, id)
	require.Equal(t, types.StatusRefunded, stored.Status)

	events := s.events(types.EventRefunded)
	require.Len(t, events, 1)
	require.Equal(t, fmt.Sprint(id), attribute(events[0], types.AttributeRequestID))
	require.Equal(t, fmt.Sprint(fee), attribute(events[0], types.AttributeFee))

	_, err = s.k.Refund(later, id)
	require.ErrorIs(t, err, types.ErrRefunded)
	require.Equal(t, before, s.balance(s.account(requester)))

	length := uint32(len(response))
	_, _, err = s.k.Fulfil(later, id, response, digest, length,
		s.signed(id, response, digest, length, s.attestors[0], s.attestors[1]))
	require.ErrorIs(t, err, types.ErrRefunded, "a refunded request is never fulfilled")
}

func TestRefundRefusals(t *testing.T) {
	s := newSuite(t, true)
	_, err := s.k.Refund(s.ctx, 1)
	require.ErrorIs(t, err, types.ErrUnknownRequest)

	id := s.request()
	length := uint32(len(response))
	_, _, err = s.k.Fulfil(s.ctx, id, response, digest, length,
		s.signed(id, response, digest, length, s.attestors[0], s.attestors[1]))
	require.NoError(t, err)
	_, err = s.k.Refund(s.ctx.WithBlockHeight(startHeight+int64(timeout)+10), id)
	require.ErrorIs(t, err, types.ErrAlreadyFulfilled)
}

func TestRefundKeepsTheStoredTimeoutAndWorksWhilePaused(t *testing.T) {
	s := newSuite(t, true)
	id := s.request()
	require.NoError(t, s.k.UpdateParams(s.ctx, types.MsgSetParams{Authority: authority, Fee: sdk.NewInt(fee),
		MaxPayloadBytes: payloadCap, MaxCallbackGas: callbackCap, TimeoutBlocks: timeout * 10}))
	require.NoError(t, s.k.Pause(s.ctx, types.MsgPause{Authority: authority}))
	_, err := s.k.Refund(s.ctx.WithBlockHeight(startHeight+int64(timeout)), id)
	require.NoError(t, err)
}
