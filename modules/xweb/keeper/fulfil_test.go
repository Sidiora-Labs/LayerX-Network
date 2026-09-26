package keeper_test

import (
	"fmt"
	"math/big"
	"strings"
	"testing"

	"github.com/ethereum/go-ethereum/crypto"
	xwebtestutil "github.com/sidiora-labs/paxeer-network/modules/xweb/testutil"
	"github.com/sidiora-labs/paxeer-network/modules/xweb/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/stretchr/testify/require"
)

func TestFulfilAtThresholdStoresTheResultAndSplitsTheFee(t *testing.T) {
	s := newSuite(t, true)
	id := s.request()
	fullLength := uint32(9000)
	signers := []xwebtestutil.Attestor{s.attestors[0], s.attestors[2]}
	request, result, err := s.k.Fulfil(s.ctx.WithBlockHeight(startHeight+5), id, response, digest, fullLength,
		s.signed(id, response, digest, fullLength, signers...))
	require.NoError(t, err)
	require.Equal(t, types.StatusFulfilled, request.Status)
	require.Equal(t, callbackGas, request.CallbackGas)

	stored, found := s.k.GetResult(s.ctx, id)
	require.True(t, found)
	require.Equal(t, result, stored)
	require.Equal(t, types.Result{
		RequestID:     id,
		Response:      response,
		ContentDigest: digest,
		FullLength:    fullLength,
		Signers:       []types.Address20{signers[0].Signer, signers[1].Signer},
		Height:        startHeight + 5,
		Callback:      types.CallbackPending,
	}, stored)
	request, _ = s.k.GetRequest(s.ctx, id)
	require.Equal(t, types.StatusFulfilled, request.Status)

	// 1_000_003 split two ways: 500_001 each, the remainder 1 to the lowest signer.
	require.Equal(t, sdk.NewInt(500_002), s.balance(signers[0].Payout))
	require.Equal(t, sdk.NewInt(500_001), s.balance(signers[1].Payout))
	require.True(t, s.balance(s.attestors[1].Payout).IsZero())
	require.True(t, s.balance(s.k.ModuleAddress()).IsZero())

	events := s.events(types.EventFulfilled)
	require.Len(t, events, 1)
	require.Equal(t, fmt.Sprint(id), attribute(events[0], types.AttributeRequestID))
	require.Equal(t, digest.Hex(), attribute(events[0], types.AttributeContentDigest))
	require.Equal(t, types.Keccak(response).Hex(), attribute(events[0], types.AttributeResponseHash))
	require.Equal(t, fmt.Sprint(fullLength), attribute(events[0], types.AttributeFullLength))
	require.Equal(t, strings.Join([]string{signers[0].Signer.Hex(), signers[1].Signer.Hex()}, ","),
		attribute(events[0], types.AttributeSigners))
}

func TestFulfilByEveryAttestorSplitsThreeWays(t *testing.T) {
	s := newSuite(t, true)
	id := s.request()
	_, _, err := s.k.Fulfil(s.ctx, id, response, digest, uint32(len(response)),
		s.signed(id, response, digest, uint32(len(response)), s.attestors...))
	require.NoError(t, err)
	// 1_000_003 / 3 = 333_334 remainder 1.
	require.Equal(t, sdk.NewInt(333_335), s.balance(s.attestors[0].Payout))
	require.Equal(t, sdk.NewInt(333_334), s.balance(s.attestors[1].Payout))
	require.Equal(t, sdk.NewInt(333_334), s.balance(s.attestors[2].Payout))
	require.True(t, s.balance(s.k.ModuleAddress()).IsZero())
}

func TestFulfilKeeperAttestationMatchesTheIndependentPreimage(t *testing.T) {
	s := newSuite(t, true)
	id := s.request()
	request, _ := s.k.GetRequest(s.ctx, id)
	require.Equal(t, s.attestation(id, response, digest, 77), s.k.Attestation(s.ctx, request, response, digest, 77))
}

func highS(signature []byte) []byte {
	out := append([]byte(nil), signature...)
	s := new(big.Int).SetBytes(out[32:64])
	flipped := new(big.Int).Sub(crypto.S256().Params().N, s)
	flipped.FillBytes(out[32:64])
	if out[64] == 27 {
		out[64] = 28
	} else {
		out[64] = 27
	}
	return out
}

func TestFulfilSignatureRefusals(t *testing.T) {
	s := newSuite(t, true)
	id := s.request()
	length := uint32(len(response))
	a, b, c := s.attestors[0], s.attestors[1], s.attestors[2]
	var outsider xwebtestutil.Attestor
	for _, candidate := range xwebtestutil.Attestors(5) {
		if !s.k.GetAttestorSet(s.ctx).Has(candidate.Signer) {
			outsider = candidate
		}
	}
	require.NotNil(t, outsider.Key)

	sign := func(attestors ...xwebtestutil.Attestor) [][]byte {
		return s.signed(id, response, digest, length, attestors...)
	}
	lowV := sign(a, b)
	lowV[0][64] -= 27
	short := sign(a, b)
	short[0] = short[0][:64]
	tooMany := make([][]byte, types.MaxAttestors+1)
	for i := range tooMany {
		tooMany[i] = sign(a)[0]
	}
	wrongDigest := append(xwebtestutil.Sign(types.Digest(s.attestation(id, []byte("other"), digest, length)), a), sign(b)...)

	for name, tc := range map[string]struct {
		signatures [][]byte
		err        error
	}{
		"none":              {nil, types.ErrBelowThreshold},
		"one of two":        {sign(a), types.ErrBelowThreshold},
		"descending order":  {sign(b, a), types.ErrBadSignature},
		"repeated signer":   {sign(a, a), types.ErrBadSignature},
		"non-attestor":      {sign(a, outsider), types.ErrBadSignature},
		"v not 27 or 28":    {lowV, types.ErrBadSignature},
		"high s":            {[][]byte{highS(sign(a)[0]), sign(b)[0]}, types.ErrBadSignature},
		"64-byte signature": {short, types.ErrBadSignature},
		"over the maximum":  {tooMany, types.ErrBadSignature},
		"different digest":  {wrongDigest, types.ErrBadSignature},
	} {
		_, _, err := s.k.Fulfil(s.ctx, id, response, digest, length, tc.signatures)
		require.ErrorIs(t, err, tc.err, name)
	}
	// Signatures over another content digest or full length do not verify.
	_, _, err := s.k.Fulfil(s.ctx, id, response, types.Hash32{0x99}, length, sign(a, b))
	require.ErrorIs(t, err, types.ErrBadSignature)
	_, _, err = s.k.Fulfil(s.ctx, id, response, digest, length+1, sign(a, b))
	require.ErrorIs(t, err, types.ErrBadSignature)

	request, _ := s.k.GetRequest(s.ctx, id)
	require.Equal(t, types.StatusPending, request.Status, "a refused fulfilment changes nothing")
	require.Equal(t, sdk.NewInt(fee), s.balance(s.k.ModuleAddress()))
	require.Empty(t, s.events(types.EventFulfilled))

	_, _, err = s.k.Fulfil(s.ctx, id, response, digest, length, sign(b, c))
	require.NoError(t, err)
}

func TestFulfilSizeRefusals(t *testing.T) {
	s := newSuite(t, true)
	id := s.request()
	oversized := make([]byte, types.MaxResponseBytes+1)
	_, _, err := s.k.Fulfil(s.ctx, id, oversized, digest, uint32(len(oversized)),
		s.signed(id, oversized, digest, uint32(len(oversized)), s.attestors[0], s.attestors[1]))
	require.ErrorIs(t, err, types.ErrResponseTooLarge)

	short := uint32(len(response) - 1)
	_, _, err = s.k.Fulfil(s.ctx, id, response, digest, short,
		s.signed(id, response, digest, short, s.attestors[0], s.attestors[1]))
	require.ErrorIs(t, err, types.ErrInvalidLength)

	bound := make([]byte, types.MaxResponseBytes)
	for i := range bound {
		bound[i] = 'x'
	}
	_, result, err := s.k.Fulfil(s.ctx, id, bound, digest, 1_000_000,
		s.signed(id, bound, digest, 1_000_000, s.attestors[0], s.attestors[1]))
	require.NoError(t, err)
	require.Len(t, result.Response, types.MaxResponseBytes)
	require.Equal(t, uint32(1_000_000), result.FullLength)
}

func TestFulfilStateRefusals(t *testing.T) {
	s := newSuite(t, true)
	id := s.request()
	length := uint32(len(response))
	signatures := s.signed(id, response, digest, length, s.attestors[0], s.attestors[1])

	_, _, err := s.k.Fulfil(s.ctx, id+1, response, digest, length, signatures)
	require.ErrorIs(t, err, types.ErrUnknownRequest)

	require.NoError(t, s.k.Pause(s.ctx, types.MsgPause{Authority: authority}))
	_, _, err = s.k.Fulfil(s.ctx, id, response, digest, length, signatures)
	require.ErrorIs(t, err, types.ErrPaused)
	require.NoError(t, s.k.Unpause(s.ctx, types.MsgUnpause{Authority: authority}))

	_, _, err = s.k.Fulfil(s.ctx, id, response, digest, length, signatures)
	require.NoError(t, err)
	_, _, err = s.k.Fulfil(s.ctx, id, response, digest, length, signatures)
	require.ErrorIs(t, err, types.ErrAlreadyFulfilled)
}

func TestRecordCallback(t *testing.T) {
	s := newSuite(t, true)
	id := s.request()
	length := uint32(len(response))
	require.ErrorIs(t, s.k.RecordCallback(s.ctx, id, types.CallbackDelivered, 1), types.ErrUnknownRequest)
	_, _, err := s.k.Fulfil(s.ctx, id, response, digest, length,
		s.signed(id, response, digest, length, s.attestors[0], s.attestors[1]))
	require.NoError(t, err)

	require.ErrorIs(t, s.k.RecordCallback(s.ctx, id, types.CallbackPending, 0), types.ErrInvalidRequest)
	require.ErrorIs(t, s.k.RecordCallback(s.ctx, id, types.CallbackOutOfGas+1, 0), types.ErrInvalidRequest)
	require.NoError(t, s.k.RecordCallback(s.ctx, id, types.CallbackOutOfGas, callbackGas))
	require.ErrorIs(t, s.k.RecordCallback(s.ctx, id, types.CallbackDelivered, 1), types.ErrCallbackRecorded)

	result, _ := s.k.GetResult(s.ctx, id)
	require.Equal(t, types.CallbackOutOfGas, result.Callback)
	require.Equal(t, callbackGas, result.CallbackGasUsed)
	request, _ := s.k.GetRequest(s.ctx, id)
	require.Equal(t, types.StatusFulfilled, request.Status, "a failed callback does not undo the fulfilment")
	require.Len(t, s.events(types.EventCallback), 1)
}
