package types_test

import (
	"testing"

	"github.com/ethereum/go-ethereum/crypto"
	"github.com/sidiora-labs/paxeer-network/modules/xweb/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/stretchr/testify/require"
)

var (
	authority = types.DefaultAuthority()
	signer    = types.Address20{0x01, 0x02}
	payout    = sdk.AccAddress(append([]byte{0xb0}, make([]byte, 19)...)).String()
)

func TestMsgValidateBasic(t *testing.T) {
	attestor := types.Attestor{Signer: signer, Payout: payout}
	require.NoError(t, types.MsgRegisterAttestor{Authority: authority, Attestor: attestor}.ValidateBasic())
	require.ErrorIs(t, types.MsgRegisterAttestor{Authority: "nope", Attestor: attestor}.ValidateBasic(), types.ErrUnauthorized)
	require.ErrorIs(t, types.MsgRegisterAttestor{Authority: authority,
		Attestor: types.Attestor{Payout: payout}}.ValidateBasic(), types.ErrInvalidAttestors)
	require.ErrorIs(t, types.MsgRegisterAttestor{Authority: authority,
		Attestor: types.Attestor{Signer: signer, Payout: "nope"}}.ValidateBasic(), types.ErrInvalidAttestors)

	require.NoError(t, types.MsgRemoveAttestor{Authority: authority, Signer: signer}.ValidateBasic())
	require.ErrorIs(t, types.MsgRemoveAttestor{Authority: authority}.ValidateBasic(), types.ErrInvalidAttestors)
	require.ErrorIs(t, types.MsgRemoveAttestor{Signer: signer}.ValidateBasic(), types.ErrUnauthorized)

	require.NoError(t, types.MsgSetThreshold{Authority: authority, Threshold: 1}.ValidateBasic())
	require.ErrorIs(t, types.MsgSetThreshold{Authority: authority}.ValidateBasic(), types.ErrInvalidThreshold)

	valid := types.MsgSetParams{Authority: authority, Fee: sdk.NewInt(5), MaxPayloadBytes: 100, MaxCallbackGas: 100, TimeoutBlocks: 10}
	require.NoError(t, valid.ValidateBasic())
	for name, mutate := range map[string]func(*types.MsgSetParams){
		"zero fee":          func(m *types.MsgSetParams) { m.Fee = sdk.ZeroInt() },
		"nil fee":           func(m *types.MsgSetParams) { m.Fee = sdk.Int{} },
		"zero payload cap":  func(m *types.MsgSetParams) { m.MaxPayloadBytes = 0 },
		"payload cap limit": func(m *types.MsgSetParams) { m.MaxPayloadBytes = types.PayloadBytesLimit + 1 },
		"zero callback cap": func(m *types.MsgSetParams) { m.MaxCallbackGas = 0 },
		"callback limit":    func(m *types.MsgSetParams) { m.MaxCallbackGas = types.CallbackGasLimit + 1 },
		"zero timeout":      func(m *types.MsgSetParams) { m.TimeoutBlocks = 0 },
	} {
		msg := valid
		mutate(&msg)
		require.ErrorIs(t, msg.ValidateBasic(), types.ErrInvalidParams, name)
	}
	bad := valid
	bad.Authority = ""
	require.ErrorIs(t, bad.ValidateBasic(), types.ErrUnauthorized)

	require.NoError(t, types.MsgPause{Authority: authority}.ValidateBasic())
	require.ErrorIs(t, types.MsgPause{}.ValidateBasic(), types.ErrUnauthorized)
	require.NoError(t, types.MsgUnpause{Authority: authority}.ValidateBasic())
	require.ErrorIs(t, types.MsgUnpause{}.ValidateBasic(), types.ErrUnauthorized)
}

func TestAttestorSetMajorityRule(t *testing.T) {
	attestors := func(n int) []types.Attestor {
		out := make([]types.Attestor, n)
		for i := range out {
			out[i] = types.Attestor{Signer: types.Address20{byte(i + 1)}, Payout: payout}
		}
		return out
	}
	require.NoError(t, types.AttestorSet{}.Validate())
	require.ErrorIs(t, types.AttestorSet{Threshold: 1}.Validate(), types.ErrInvalidAttestors)
	for _, tc := range []struct {
		n         int
		threshold uint32
		ok        bool
	}{
		{1, 1, true}, {1, 0, false}, {1, 2, false},
		{2, 1, false}, {2, 2, true},
		{3, 1, false}, {3, 2, true}, {3, 3, true}, {3, 4, false},
		{4, 2, false}, {4, 3, true},
	} {
		err := types.AttestorSet{Attestors: attestors(tc.n), Threshold: tc.threshold}.Validate()
		if tc.ok {
			require.NoError(t, err, "%d of %d", tc.threshold, tc.n)
		} else {
			require.ErrorIs(t, err, types.ErrInvalidThreshold, "%d of %d", tc.threshold, tc.n)
		}
	}
	require.Equal(t, uint32(1), types.Majority(1))
	require.Equal(t, uint32(2), types.Majority(2))
	require.Equal(t, uint32(2), types.Majority(3))
	require.Equal(t, uint32(3), types.Majority(4))

	duplicate := attestors(2)
	duplicate[1].Signer = duplicate[0].Signer
	require.ErrorIs(t, types.AttestorSet{Attestors: duplicate, Threshold: 2}.Validate(), types.ErrInvalidAttestors)
	require.ErrorIs(t, types.AttestorSet{Attestors: attestors(types.MaxAttestors + 1), Threshold: types.MaxAttestors}.Validate(),
		types.ErrInvalidAttestors)
}

func TestDefaultGenesisIsPausedAndEmpty(t *testing.T) {
	genesis := types.DefaultGenesis()
	require.NoError(t, genesis.Validate())
	require.True(t, genesis.Paused)
	require.Empty(t, genesis.Attestors.Attestors)
	require.Zero(t, genesis.Attestors.Threshold)
	require.Equal(t, types.DefaultParams(authority), genesis.Params)
	require.Equal(t, types.DefaultFee(), genesis.Params.Fee)
	require.Equal(t, types.DefaultMaxPayloadBytes, genesis.Params.MaxPayloadBytes)
	require.Equal(t, types.DefaultMaxCallbackGas, genesis.Params.MaxCallbackGas)
	require.Equal(t, types.DefaultTimeoutBlocks, genesis.Params.TimeoutBlocks)
}

func TestGenesisValidateRefusesInconsistentRecords(t *testing.T) {
	request := types.Request{ID: 1, Requester: types.Address20{0x0a}, Kind: types.KindFetch, CallbackGas: 1,
		Fee: sdk.NewInt(1), Height: 5, TimeoutHeight: 10, Status: types.StatusFulfilled}
	result := types.Result{RequestID: 1, Response: []byte("x"), FullLength: 1, Signers: []types.Address20{signer}}
	base := func() types.GenesisState {
		g := *types.DefaultGenesis()
		g.Nonce = 1
		g.Requests = []types.Request{request}
		g.Results = []types.Result{result}
		return g
	}
	good := base()
	require.NoError(t, good.Validate())

	for name, mutate := range map[string]func(*types.GenesisState){
		"request above nonce": func(g *types.GenesisState) { g.Nonce = 0 },
		"duplicate request":   func(g *types.GenesisState) { g.Requests = append(g.Requests, request) },
		"duplicate result":    func(g *types.GenesisState) { g.Results = append(g.Results, result) },
		"fulfilled no result": func(g *types.GenesisState) { g.Results = nil },
		"result of pending": func(g *types.GenesisState) {
			g.Requests[0].Status = types.StatusPending
		},
		"oversized result": func(g *types.GenesisState) {
			g.Results[0].Response = make([]byte, types.MaxResponseBytes+1)
			g.Results[0].FullLength = types.MaxResponseBytes + 1
		},
		"short length":   func(g *types.GenesisState) { g.Results[0].FullLength = 0 },
		"unknown kind":   func(g *types.GenesisState) { g.Requests[0].Kind = 9 },
		"bad timeout":    func(g *types.GenesisState) { g.Requests[0].TimeoutHeight = 5 },
		"bad attestors":  func(g *types.GenesisState) { g.Attestors.Threshold = 1 },
		"bad parameters": func(g *types.GenesisState) { g.Params.TimeoutBlocks = 0 },
		"single level on fetch": func(g *types.GenesisState) {
			g.Requests[0].Level = types.LevelSingle
			g.Requests[0].Attestor = signer
		},
		"majority naming an attestor": func(g *types.GenesisState) { g.Requests[0].Attestor = signer },
		"unknown request level":       func(g *types.GenesisState) { g.Requests[0].Level = 2 },
		"unknown result level":        func(g *types.GenesisState) { g.Results[0].Level = 2 },
		"single result with two signers": func(g *types.GenesisState) {
			g.Results[0].Level = types.LevelSingle
			g.Results[0].Signers = []types.Address20{signer, {0x09}}
		},
	} {
		g := base()
		g.Requests = append([]types.Request(nil), g.Requests...)
		g.Results = append([]types.Result(nil), g.Results...)
		mutate(&g)
		require.Error(t, g.Validate(), name)
	}
}

func TestRequestAndResultLevels(t *testing.T) {
	request := types.Request{ID: 1, Requester: types.Address20{0x0a}, Kind: types.KindApi, CallbackGas: 1,
		Fee: sdk.NewInt(1), Height: 5, TimeoutHeight: 10}
	require.NoError(t, request.Validate())

	single := request
	single.Level = types.LevelSingle
	single.Attestor = signer
	require.NoError(t, single.Validate())

	unnamed := single
	unnamed.Attestor = types.Address20{}
	require.ErrorIs(t, unnamed.Validate(), types.ErrInvalidLevel)
	require.ErrorContains(t, unnamed.Validate(), "single level names no attestor")

	search := single
	search.Kind = types.KindSearch
	require.ErrorContains(t, search.Validate(), "single level on kind 2, open to the api kind only")

	named := request
	named.Attestor = signer
	require.ErrorContains(t, named.Validate(), "majority level names attestor "+signer.Hex())

	unknown := request
	unknown.Level = 7
	require.ErrorContains(t, unknown.Validate(), "level 7")

	result := types.Result{RequestID: 1, Response: []byte("x"), FullLength: 1, Signers: []types.Address20{signer},
		Level: types.LevelSingle}
	require.NoError(t, result.Validate())
	result.Signers = append(result.Signers, types.Address20{0x09})
	require.ErrorIs(t, result.Validate(), types.ErrInvalidLevel)
	result.Level = types.LevelMajority
	require.NoError(t, result.Validate())
	result.Level = 3
	require.ErrorIs(t, result.Validate(), types.ErrInvalidLevel)
}

func TestAttestorPublicKey(t *testing.T) {
	key, err := crypto.ToECDSA(crypto.Keccak256([]byte("PAXEERX_WEB_API_ENVELOPE_V1 vector attestor 1")))
	require.NoError(t, err)
	other, err := crypto.ToECDSA(crypto.Keccak256([]byte("PAXEERX_WEB_API_ENVELOPE_V1 vector attestor 2")))
	require.NoError(t, err)
	address := types.Address20(crypto.PubkeyToAddress(key.PublicKey))
	compressed := crypto.CompressPubkey(&key.PublicKey)

	require.NoError(t, types.Attestor{Signer: address, Payout: payout}.Validate())
	require.NoError(t, types.Attestor{Signer: address, Payout: payout, PublicKey: compressed}.Validate())

	for name, tc := range map[string]struct {
		key     []byte
		refuses string
	}{
		"uncompressed": {crypto.FromECDSAPub(&key.PublicKey), "is 65 bytes, want 33 compressed"},
		"short":        {compressed[:32], "is 32 bytes, want 33 compressed"},
		"not a point":  {append([]byte{0x05}, compressed[1:]...), "public key of " + address.Hex()},
		"another key": {crypto.CompressPubkey(&other.PublicKey),
			"belongs to " + types.Address20(crypto.PubkeyToAddress(other.PublicKey)).Hex()},
	} {
		err := types.Attestor{Signer: address, Payout: payout, PublicKey: tc.key}.Validate()
		require.ErrorIs(t, err, types.ErrInvalidAttestors, name)
		require.ErrorContains(t, err, tc.refuses, name)
		err = types.MsgRegisterAttestor{Authority: authority,
			Attestor: types.Attestor{Signer: address, Payout: payout, PublicKey: tc.key}}.ValidateBasic()
		require.ErrorIs(t, err, types.ErrInvalidAttestors, name)
	}
}
