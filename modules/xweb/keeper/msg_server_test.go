package keeper_test

import (
	"bytes"
	"encoding/hex"
	"testing"

	"github.com/ethereum/go-ethereum/crypto"
	xwebtestutil "github.com/sidiora-labs/paxeer-network/modules/xweb/testutil"
	"github.com/sidiora-labs/paxeer-network/modules/xweb/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/stretchr/testify/require"
)

func (s *suite) register(attestor xwebtestutil.Attestor) error {
	return s.k.RegisterAttestor(s.ctx, types.MsgRegisterAttestor{Authority: authority, Attestor: attestor.Registration()})
}

func TestGovernanceMessagesRequireTheAuthority(t *testing.T) {
	s := newSuite(t, false)
	stranger := sdk.AccAddress(make([]byte, 20)).String()
	attestor := s.attestors[0]
	require.ErrorIs(t, s.k.RegisterAttestor(s.ctx, types.MsgRegisterAttestor{Authority: stranger,
		Attestor: attestor.Registration()}), types.ErrUnauthorized)
	require.NoError(t, s.register(attestor))
	require.ErrorIs(t, s.k.RemoveAttestor(s.ctx, types.MsgRemoveAttestor{Authority: stranger, Signer: attestor.Signer}),
		types.ErrUnauthorized)
	require.ErrorIs(t, s.k.SetThreshold(s.ctx, types.MsgSetThreshold{Authority: stranger, Threshold: 1}), types.ErrUnauthorized)
	require.ErrorIs(t, s.k.UpdateParams(s.ctx, types.MsgSetParams{Authority: stranger, Fee: sdk.NewInt(1),
		MaxPayloadBytes: 1, MaxCallbackGas: 1, TimeoutBlocks: 1}), types.ErrUnauthorized)
	require.ErrorIs(t, s.k.Pause(s.ctx, types.MsgPause{Authority: stranger}), types.ErrUnauthorized)
	require.ErrorIs(t, s.k.Unpause(s.ctx, types.MsgUnpause{Authority: stranger}), types.ErrUnauthorized)
	require.ErrorIs(t, s.k.Unpause(s.ctx, types.MsgUnpause{}), types.ErrUnauthorized)
	require.True(t, s.k.IsPaused(s.ctx))
	require.Len(t, s.k.GetAttestorSet(s.ctx).Attestors, 1)
}

func TestRegisterAttestorRaisesTheThresholdToTheMajority(t *testing.T) {
	s := newSuite(t, false)
	attestors := xwebtestutil.Attestors(5)
	for i, want := range []uint32{1, 2, 2, 3, 3} {
		require.NoError(t, s.register(attestors[len(attestors)-1-i]))
		require.Equal(t, want, s.k.Threshold(s.ctx), "after %d registrations", i+1)
	}
	set := s.k.GetAttestorSet(s.ctx)
	require.NoError(t, set.Validate())
	for i := 1; i < len(set.Attestors); i++ {
		require.Negative(t, bytes.Compare(set.Attestors[i-1].Signer[:], set.Attestors[i].Signer[:]), "ascending signers")
	}
	require.Len(t, s.events(types.EventAttestorAdded), 5)

	require.NoError(t, s.k.SetThreshold(s.ctx, types.MsgSetThreshold{Authority: authority, Threshold: 5}))
	extra := xwebtestutil.Attestors(6)
	for _, candidate := range extra {
		if !set.Has(candidate.Signer) {
			require.NoError(t, s.register(candidate))
			break
		}
	}
	require.Equal(t, uint32(5), s.k.Threshold(s.ctx), "a threshold above the majority is kept")

	require.ErrorIs(t, s.register(attestors[0]), types.ErrInvalidAttestors, "duplicate")
	require.ErrorIs(t, s.k.RegisterAttestor(s.ctx, types.MsgRegisterAttestor{Authority: authority,
		Attestor: types.Attestor{Signer: types.Address20{0x01}, Payout: "nope"}}), types.ErrInvalidAttestors)
}

func TestRegisterAttestorRefusesAFullSet(t *testing.T) {
	s := newSuite(t, false)
	attestors := xwebtestutil.Attestors(types.MaxAttestors + 1)
	for _, attestor := range attestors[:types.MaxAttestors] {
		require.NoError(t, s.register(attestor))
	}
	require.ErrorIs(t, s.register(attestors[types.MaxAttestors]), types.ErrInvalidAttestors)
	require.Equal(t, types.Majority(types.MaxAttestors), s.k.Threshold(s.ctx))
}

func TestRemoveAttestor(t *testing.T) {
	s := newSuite(t, true)
	a, b, c := s.attestors[0], s.attestors[1], s.attestors[2]
	require.ErrorIs(t, s.k.RemoveAttestor(s.ctx, types.MsgRemoveAttestor{Authority: authority,
		Signer: types.Address20{0x01}}), types.ErrUnknownAttestor)

	require.NoError(t, s.k.SetThreshold(s.ctx, types.MsgSetThreshold{Authority: authority, Threshold: 3}))
	require.NoError(t, s.k.RemoveAttestor(s.ctx, types.MsgRemoveAttestor{Authority: authority, Signer: b.Signer}))
	set := s.k.GetAttestorSet(s.ctx)
	require.Equal(t, uint32(2), set.Threshold, "a threshold above the set drops to the set size")
	require.False(t, set.Has(b.Signer))
	require.True(t, set.Has(a.Signer))
	require.True(t, set.Has(c.Signer))

	id := s.request()
	length := uint32(len(response))
	_, _, err := s.k.Fulfil(s.ctx, id, response, digest, length, s.signed(id, response, digest, length, a, b))
	require.ErrorIs(t, err, types.ErrBadSignature, "a removed attestor's signature is refused")

	require.NoError(t, s.k.RemoveAttestor(s.ctx, types.MsgRemoveAttestor{Authority: authority, Signer: a.Signer}))
	require.Equal(t, uint32(1), s.k.Threshold(s.ctx))
	require.NoError(t, s.k.RemoveAttestor(s.ctx, types.MsgRemoveAttestor{Authority: authority, Signer: c.Signer}))
	require.Empty(t, s.k.GetAttestorSet(s.ctx).Attestors)
	require.Zero(t, s.k.Threshold(s.ctx))
	require.Len(t, s.events(types.EventAttestorRemoved), 3)
}

func TestSetThresholdMajorityRule(t *testing.T) {
	s := newSuite(t, false)
	require.ErrorIs(t, s.k.SetThreshold(s.ctx, types.MsgSetThreshold{Authority: authority, Threshold: 1}),
		types.ErrInvalidThreshold, "no attestors")
	attestors := xwebtestutil.Attestors(4)
	for _, attestor := range attestors {
		require.NoError(t, s.register(attestor))
	}
	require.Equal(t, uint32(3), s.k.Threshold(s.ctx))
	for _, threshold := range []uint32{1, 2, 5} {
		require.ErrorIs(t, s.k.SetThreshold(s.ctx, types.MsgSetThreshold{Authority: authority, Threshold: threshold}),
			types.ErrInvalidThreshold, "threshold %d of 4", threshold)
	}
	require.ErrorIs(t, s.k.SetThreshold(s.ctx, types.MsgSetThreshold{Authority: authority}), types.ErrInvalidThreshold)
	require.Equal(t, uint32(3), s.k.Threshold(s.ctx))
	require.NoError(t, s.k.SetThreshold(s.ctx, types.MsgSetThreshold{Authority: authority, Threshold: 4}))
	require.Equal(t, uint32(4), s.k.Threshold(s.ctx))
	require.Len(t, s.events(types.EventThresholdSet), 1)
}

func TestUpdateParams(t *testing.T) {
	s := newSuite(t, false)
	msg := types.MsgSetParams{Authority: authority, Fee: sdk.NewInt(42), MaxPayloadBytes: 64, MaxCallbackGas: 90_000,
		TimeoutBlocks: 7}
	require.NoError(t, s.k.UpdateParams(s.ctx, msg))
	require.Equal(t, types.Params{Authority: authority, Fee: sdk.NewInt(42), MaxPayloadBytes: 64, MaxCallbackGas: 90_000,
		TimeoutBlocks: 7}, s.k.GetParams(s.ctx))
	require.Equal(t, sdk.NewInt(42), s.k.Fee(s.ctx))
	require.Len(t, s.events(types.EventParamsSet), 1)

	for name, mutate := range map[string]func(*types.MsgSetParams){
		"zero fee":          func(m *types.MsgSetParams) { m.Fee = sdk.ZeroInt() },
		"negative fee":      func(m *types.MsgSetParams) { m.Fee = sdk.NewInt(-1) },
		"zero payload cap":  func(m *types.MsgSetParams) { m.MaxPayloadBytes = 0 },
		"payload cap limit": func(m *types.MsgSetParams) { m.MaxPayloadBytes = types.PayloadBytesLimit + 1 },
		"zero callback cap": func(m *types.MsgSetParams) { m.MaxCallbackGas = 0 },
		"callback limit":    func(m *types.MsgSetParams) { m.MaxCallbackGas = types.CallbackGasLimit + 1 },
		"zero timeout":      func(m *types.MsgSetParams) { m.TimeoutBlocks = 0 },
	} {
		bad := msg
		mutate(&bad)
		require.ErrorIs(t, s.k.UpdateParams(s.ctx, bad), types.ErrInvalidParams, name)
	}
	require.Equal(t, sdk.NewInt(42), s.k.Fee(s.ctx), "a refused update changes nothing")
}

func TestPauseAndUnpause(t *testing.T) {
	s := newSuite(t, true)
	require.False(t, s.k.IsPaused(s.ctx))
	require.NoError(t, s.k.Pause(s.ctx, types.MsgPause{Authority: authority}))
	require.True(t, s.k.IsPaused(s.ctx))
	require.Len(t, s.events(types.EventPaused), 1)
	require.NoError(t, s.k.Unpause(s.ctx, types.MsgUnpause{Authority: authority}))
	require.False(t, s.k.IsPaused(s.ctx))
	require.Len(t, s.events(types.EventUnpaused), 2, "the suite unpaused once on activation")
}

func TestRegisterAttestorWithAPublicKey(t *testing.T) {
	s := newSuite(t, false)
	attestor := s.attestors[0]
	registration := attestor.Registration()
	registration.PublicKey = crypto.CompressPubkey(&attestor.Key.PublicKey)
	require.NoError(t, s.k.RegisterAttestor(s.ctx, types.MsgRegisterAttestor{Authority: authority, Attestor: registration}))
	stored, found := s.k.GetAttestorSet(s.ctx).Find(attestor.Signer)
	require.True(t, found)
	require.Equal(t, registration.PublicKey, stored.PublicKey)
	events := s.events(types.EventAttestorAdded)
	require.Len(t, events, 1)
	require.Equal(t, hex.EncodeToString(registration.PublicKey), attribute(events[0], types.AttributePublicKey))

	require.NoError(t, s.register(s.attestors[1]))
	events = s.events(types.EventAttestorAdded)
	require.Len(t, events, 2)
	require.Empty(t, attribute(events[1], types.AttributePublicKey))

	wrong := s.attestors[2].Registration()
	wrong.PublicKey = registration.PublicKey
	err := s.k.RegisterAttestor(s.ctx, types.MsgRegisterAttestor{Authority: authority, Attestor: wrong})
	require.ErrorIs(t, err, types.ErrInvalidAttestors)
	require.ErrorContains(t, err, "belongs to "+attestor.Signer.Hex())
	require.Len(t, s.k.GetAttestorSet(s.ctx).Attestors, 2)
}
