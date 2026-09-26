package keeper_test

import (
	"encoding/hex"
	"fmt"
	"testing"

	"github.com/ethereum/go-ethereum/common"
	xwebtestutil "github.com/sidiora-labs/paxeer-network/modules/xweb/testutil"
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
	require.Equal(t, "0", attribute(event, types.AttributeLevel))
	require.Equal(t, types.Address20{}.Hex(), attribute(event, types.AttributeAttestor))
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
		"kind four":         {4, payload, callbackGas, sdk.NewInt(fee), types.ErrUnknownKind},
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

const apiPayloadCap = types.DefaultMaxPayloadBytes

// apiPayload is a GET api payload with a credential envelope sealed to each of
// sealedTo, under the single level when named is non-zero.
func (s *suite) apiPayload(named types.Address20, sealedTo ...xwebtestutil.Attestor) []byte {
	s.t.Helper()
	call := types.ApiPayload{Method: types.MethodGet, URL: "https://paxeer.app/api/v1/price?asset=PAX",
		Headers: []types.ApiHeader{{Name: "Accept", Value: "application/json"}}, Pointers: []string{"/data/price"}}
	if named != (types.Address20{}) {
		call.Level, call.Attestor = types.LevelSingle, named
	}
	origin, err := call.Origin()
	require.NoError(s.t, err)
	credential, err := types.EncodeCredential([]types.ApiHeader{{Name: "X-Api-Key", Value: "paxeer-test-credential"}})
	require.NoError(s.t, err)
	for _, attestor := range sealedTo {
		sealed, err := types.SealEnvelope(&attestor.Key.PublicKey, origin, credential)
		require.NoError(s.t, err)
		envelope, err := types.ParseEnvelope(sealed)
		require.NoError(s.t, err)
		call.Envelopes = append(call.Envelopes, envelope)
	}
	encoded, err := call.Encode()
	require.NoError(s.t, err)
	return encoded
}

// allowApi raises the payload cap to the default so api payloads fit.
func (s *suite) allowApi() {
	s.t.Helper()
	require.NoError(s.t, s.k.UpdateParams(s.ctx, types.MsgSetParams{Authority: authority, Fee: sdk.NewInt(fee),
		MaxPayloadBytes: apiPayloadCap, MaxCallbackGas: callbackCap, TimeoutBlocks: timeout}))
}

func TestApiRequestStoresTheLevelAndTheNamedAttestor(t *testing.T) {
	s := newSuite(t, true)
	s.allowApi()
	named := s.attestors[1]
	body := s.apiPayload(named.Signer, named)
	id, err := s.k.Request(s.ctx, requester, types.KindApi, body, callbackGas, sdk.NewInt(fee))
	require.NoError(t, err)
	request, found := s.k.GetRequest(s.ctx, id)
	require.True(t, found)
	require.Equal(t, types.Request{
		ID:            id,
		Requester:     types.Address20(requester),
		Kind:          types.KindApi,
		PayloadHash:   types.Keccak(body),
		CallbackGas:   callbackGas,
		Fee:           sdk.NewInt(fee),
		Height:        startHeight,
		TimeoutHeight: startHeight + int64(timeout),
		Status:        types.StatusPending,
		Level:         types.LevelSingle,
		Attestor:      named.Signer,
	}, request)
	events := s.events(types.EventRequested)
	require.Len(t, events, 1)
	require.Equal(t, "3", attribute(events[0], types.AttributeKind))
	require.Equal(t, hex.EncodeToString(body), attribute(events[0], types.AttributePayload))
	require.Equal(t, "1", attribute(events[0], types.AttributeLevel))
	require.Equal(t, named.Signer.Hex(), attribute(events[0], types.AttributeAttestor))

	majority := s.apiPayload(types.Address20{}, s.attestors...)
	id, err = s.k.Request(s.ctx, requester, types.KindApi, majority, callbackGas, sdk.NewInt(fee))
	require.NoError(t, err)
	request, _ = s.k.GetRequest(s.ctx, id)
	require.Equal(t, types.LevelMajority, request.Level)
	require.Equal(t, types.Address20{}, request.Attestor)
}

func TestApiRequestRefusals(t *testing.T) {
	s := newSuite(t, true)
	s.allowApi()
	var outsider xwebtestutil.Attestor
	for _, candidate := range xwebtestutil.Attestors(5) {
		if !s.k.GetAttestorSet(s.ctx).Has(candidate.Signer) {
			outsider = candidate
		}
	}
	require.NotNil(t, outsider.Key)
	a := s.attestors[0]
	for name, tc := range map[string]struct {
		payload []byte
		err     error
		refuses string
	}{
		"a fetch payload":            {payload, types.ErrInvalidApi, "payload ends inside the version"},
		"an unregistered single":     {s.apiPayload(outsider.Signer), types.ErrUnknownAttestor, "the single level names"},
		"an envelope to an outsider": {s.apiPayload(types.Address20{}, a, outsider), types.ErrUnknownAttestor, "envelope 1 is addressed to"},
	} {
		_, err := s.k.Request(s.ctx, requester, types.KindApi, tc.payload, callbackGas, sdk.NewInt(fee))
		require.ErrorIs(t, err, tc.err, name)
		require.ErrorContains(t, err, tc.refuses, name)
	}
	// A majority payload with an envelope to another attestor, turned single
	// and naming a.
	mismatched := s.apiPayload(types.Address20{}, s.attestors[1])
	mismatched[2] = types.LevelSingle
	copy(mismatched[3:23], a.Signer[:])
	_, err := s.k.Request(s.ctx, requester, types.KindApi, mismatched, callbackGas, sdk.NewInt(fee))
	require.ErrorIs(t, err, types.ErrInvalidApi)
	require.ErrorContains(t, err, "envelope 0 is addressed to "+s.attestors[1].Signer.Hex()+", the single level names "+
		a.Signer.Hex())

	// Two envelopes of one length, the second overwritten with the first.
	pair := s.apiPayload(types.Address20{}, a, s.attestors[1])
	length := (len(pair) - len(s.apiPayload(types.Address20{}))) / 2
	envelopeLength := length - 2
	repeated := append([]byte(nil), pair...)
	copy(repeated[len(repeated)-envelopeLength:], pair[len(pair)-2*length+2:len(pair)-length])
	_, err = s.k.Request(s.ctx, requester, types.KindApi, repeated, callbackGas, sdk.NewInt(fee))
	require.ErrorIs(t, err, types.ErrInvalidApi)
	require.ErrorContains(t, err, "envelope 1 repeats attestor "+a.Signer.Hex())
	require.Zero(t, s.k.Nonce(s.ctx), "a refused api request takes no nonce")

	few := newSuite(t, false)
	require.NoError(t, few.register(few.attestors[0]))
	require.NoError(t, few.k.UpdateParams(few.ctx, types.MsgSetParams{Authority: authority, Fee: sdk.NewInt(fee),
		MaxPayloadBytes: apiPayloadCap, MaxCallbackGas: callbackCap, TimeoutBlocks: timeout}))
	require.NoError(t, few.k.Unpause(few.ctx, types.MsgUnpause{Authority: authority}))
	few.fund(requester, requesterBal)
	_, err = few.k.Request(few.ctx, requester, types.KindApi, few.apiPayload(types.Address20{}, few.attestors[0],
		few.attestors[1]), callbackGas, sdk.NewInt(fee))
	require.ErrorIs(t, err, types.ErrInvalidApi)
	require.ErrorContains(t, err, "2 envelopes for 1 registered attestors")
}
