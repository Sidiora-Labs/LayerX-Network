package keeper_test

import (
	"testing"
	"time"

	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/modules/xweb/keeper"
	xwebtestutil "github.com/sidiora-labs/paxeer-network/modules/xweb/testutil"
	"github.com/sidiora-labs/paxeer-network/modules/xweb/types"
	app "github.com/sidiora-labs/paxeer-network/node"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/stretchr/testify/require"
)

const (
	fee          = int64(1_000_003)
	callbackGas  = uint64(200_000)
	timeout      = uint64(100)
	startHeight  = int64(8)
	payloadCap   = uint32(256)
	callbackCap  = uint64(300_000)
	requesterBal = int64(10_000_000)
)

var (
	requester = common.HexToAddress("0x00000000000000000000000000000000000a11ce")
	authority = types.DefaultAuthority()
	payload   = []byte("https://paxeer.app/")
	digest    = types.Hash32{0x22, 0x22, 0x22}
	response  = []byte("Paxeer X Network")
)

type suite struct {
	t         *testing.T
	app       *app.App
	ctx       sdk.Context
	k         keeper.Keeper
	attestors []xwebtestutil.Attestor
}

// newSuite starts from the default (paused, empty) genesis. With activate it
// registers three attestors (threshold two), sets the fee, caps and timeout,
// unpauses, and funds the requester.
func newSuite(t *testing.T, activate bool) *suite {
	t.Helper()
	testApp := app.Setup(t, false, false, false)
	ctx := testApp.GetContextForDeliverTx([]byte{}).WithBlockHeight(startHeight).WithBlockTime(time.Unix(1_800_000_000, 0))
	k, ctx := xwebtestutil.NewKeeper(testApp, ctx)
	k.InitGenesis(ctx, *types.DefaultGenesis())
	s := &suite{t: t, app: testApp, ctx: ctx, k: k, attestors: xwebtestutil.Attestors(3)}
	if activate {
		for _, attestor := range s.attestors {
			require.NoError(t, k.RegisterAttestor(ctx, types.MsgRegisterAttestor{Authority: authority,
				Attestor: attestor.Registration()}))
		}
		require.NoError(t, k.UpdateParams(ctx, types.MsgSetParams{Authority: authority, Fee: sdk.NewInt(fee),
			MaxPayloadBytes: payloadCap, MaxCallbackGas: callbackCap, TimeoutBlocks: timeout}))
		require.NoError(t, k.Unpause(ctx, types.MsgUnpause{Authority: authority}))
		s.fund(requester, requesterBal)
	}
	return s
}

func (s *suite) account(address common.Address) sdk.AccAddress {
	return s.app.EvmKeeper.GetPaxAddressOrDefault(s.ctx, address)
}

func (s *suite) fund(address common.Address, amount int64) {
	s.t.Helper()
	coins := sdk.NewCoins(sdk.NewCoin(sdk.MustGetBaseDenom(), sdk.NewInt(amount)))
	require.NoError(s.t, s.app.BankKeeper.MintCoins(s.ctx, "evm", coins))
	require.NoError(s.t, s.app.BankKeeper.SendCoinsFromModuleToAccount(s.ctx, "evm", s.account(address), coins))
}

func (s *suite) balance(of sdk.AccAddress) sdk.Int {
	return s.app.BankKeeper.GetBalance(s.ctx, of, sdk.MustGetBaseDenom()).Amount
}

func (s *suite) request() uint64 {
	s.t.Helper()
	id, err := s.k.Request(s.ctx, requester, types.KindFetch, payload, callbackGas, sdk.NewInt(fee))
	require.NoError(s.t, err)
	return id
}

// attestation is the origin-1 attestation built independently of the keeper
// from the stored request and the EVM chain id.
func (s *suite) attestation(id uint64, body []byte, content types.Hash32, fullLength uint32) types.Attestation {
	request, found := s.k.GetRequest(s.ctx, id)
	require.True(s.t, found)
	return types.Attestation{
		Origin:        types.OriginEVM,
		NetworkID:     s.app.EvmKeeper.ChainID(s.ctx),
		Requester:     types.EVMRequester(types.Address20(requester)),
		RequestID:     id,
		Kind:          request.Kind,
		PayloadHash:   types.Keccak(payload),
		ContentDigest: content,
		ResponseHash:  types.Keccak(body),
		FullLength:    fullLength,
	}
}

func (s *suite) signed(id uint64, body []byte, content types.Hash32, fullLength uint32,
	attestors ...xwebtestutil.Attestor) [][]byte {
	return xwebtestutil.Sign(types.Digest(s.attestation(id, body, content, fullLength)), attestors...)
}

func (s *suite) events(kind string) []sdk.Event {
	var out []sdk.Event
	for _, event := range s.ctx.EventManager().Events() {
		if event.Type == kind {
			out = append(out, event)
		}
	}
	return out
}

func attribute(event sdk.Event, key string) string {
	for _, attr := range event.Attributes {
		if string(attr.Key) == key {
			return string(attr.Value)
		}
	}
	return ""
}

func TestDefaultGenesisIsDormant(t *testing.T) {
	s := newSuite(t, false)
	require.True(t, s.k.IsPaused(s.ctx))
	require.Empty(t, s.k.GetAttestorSet(s.ctx).Attestors)
	require.Zero(t, s.k.Threshold(s.ctx))
	require.Equal(t, types.DefaultFee(), s.k.Fee(s.ctx))
	require.Equal(t, *types.DefaultGenesis(), s.k.ExportGenesis(s.ctx))

	_, err := s.k.Request(s.ctx, requester, types.KindFetch, payload, callbackGas, types.DefaultFee())
	require.ErrorIs(t, err, types.ErrPaused)
	_, _, err = s.k.Fulfil(s.ctx, 1, response, digest, uint32(len(response)), nil)
	require.ErrorIs(t, err, types.ErrPaused)

	require.NoError(t, s.k.Unpause(s.ctx, types.MsgUnpause{Authority: authority}))
	s.fund(requester, requesterBal)
	id, err := s.k.Request(s.ctx, requester, types.KindFetch, payload, callbackGas, types.DefaultFee())
	require.NoError(t, err)
	_, _, err = s.k.Fulfil(s.ctx, id, response, digest, uint32(len(response)), nil)
	require.ErrorIs(t, err, types.ErrBelowThreshold)
}

func TestGenesisRoundTrip(t *testing.T) {
	s := newSuite(t, true)
	fulfilled := s.request()
	pending := s.request()
	_, _, err := s.k.Fulfil(s.ctx, fulfilled, response, digest, 40,
		s.signed(fulfilled, response, digest, 40, s.attestors[0], s.attestors[1]))
	require.NoError(t, err)
	require.NoError(t, s.k.RecordCallback(s.ctx, fulfilled, types.CallbackReverted, 21_000))

	exported := s.k.ExportGenesis(s.ctx)
	require.NoError(t, exported.Validate())
	require.False(t, exported.Paused)
	require.Equal(t, uint64(2), exported.Nonce)
	require.Len(t, exported.Requests, 2)
	require.Len(t, exported.Results, 1)
	require.Equal(t, types.StatusPending, exported.Requests[1].Status)
	require.Equal(t, pending, exported.Requests[1].ID)

	fresh := newSuite(t, false)
	fresh.k.InitGenesis(fresh.ctx, exported)
	require.Equal(t, exported, fresh.k.ExportGenesis(fresh.ctx))
	result, found := fresh.k.GetResult(fresh.ctx, fulfilled)
	require.True(t, found)
	require.Equal(t, response, result.Response)
	require.Equal(t, types.CallbackReverted, result.Callback)
	require.Equal(t, uint64(21_000), result.CallbackGasUsed)
}

func TestInitGenesisPanicsOnInvalidState(t *testing.T) {
	s := newSuite(t, false)
	genesis := *types.DefaultGenesis()
	genesis.Attestors.Threshold = 1
	require.Panics(t, func() { s.k.InitGenesis(s.ctx, genesis) })
}

func TestModuleAddressIsTheXWebAccount(t *testing.T) {
	s := newSuite(t, false)
	require.Equal(t, types.ModuleAddress(), s.k.ModuleAddress())
	require.Equal(t, "0x0000000000000000000000000000000000001019", types.PrecompileAddress)
	require.True(t, s.balance(s.k.ModuleAddress()).IsZero())
}
