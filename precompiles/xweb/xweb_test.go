package xweb_test

import (
	"bytes"
	"maps"
	"math"
	"math/big"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/ethereum/go-ethereum/accounts/abi"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core"
	ethtypes "github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/core/vm"
	"github.com/ethereum/go-ethereum/crypto"
	"github.com/holiman/uint256"
	"github.com/sidiora-labs/paxeer-network/modules/evm/state"
	evmtypes "github.com/sidiora-labs/paxeer-network/modules/evm/types"
	xwebkeeper "github.com/sidiora-labs/paxeer-network/modules/xweb/keeper"
	xwebtestutil "github.com/sidiora-labs/paxeer-network/modules/xweb/testutil"
	"github.com/sidiora-labs/paxeer-network/modules/xweb/types"
	"github.com/sidiora-labs/paxeer-network/precompiles/utils"
	"github.com/sidiora-labs/paxeer-network/precompiles/xweb"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

const (
	fee         = int64(1_000_003)
	callbackGas = uint64(200_000)
	timeout     = uint64(100)
	startHeight = int64(8)
	payloadCap  = uint32(256)
	callbackCap = uint64(300_000)
	supplied    = uint64(2_000_000)
)

var (
	precompile = common.HexToAddress(xweb.XWebAddress)
	authority  = types.DefaultAuthority()
	payload    = []byte("https://paxeer.app/")
	digest     = [32]byte{0x22, 0x22, 0x22}
	response   = []byte("Paxeer X Network")
	fullLength = uint32(4096)

	// recorder stores the request id in slot 0, its caller in slot 1 and the
	// content digest in slot 2, then stops.
	recorderCode = common.FromHex("6004356000553360015560243560025500")
	// reverter reverts with no data.
	reverterCode = common.FromHex("60006000fd")
	// burner loops until it runs out of gas.
	burnerCode = common.FromHex("5b600056")

	recorder = common.HexToAddress("0x00000000000000000000000000000000c0de0001")
	reverter = common.HexToAddress("0x00000000000000000000000000000000c0de0002")
	burner   = common.HexToAddress("0x00000000000000000000000000000000c0de0003")
	stranger = common.HexToAddress("0x00000000000000000000000000000000c0de0004")
)

type keepers struct {
	utils.Keepers
	xweb *xwebkeeper.Keeper
}

func (k keepers) XWebK() *xwebkeeper.Keeper { return k.xweb }

type harness struct {
	t          *testing.T
	stateDB    *state.DBImpl
	evm        *vm.EVM
	precompile *xweb.Precompile
	keeper     *xwebkeeper.Keeper
	attestors  []xwebtestutil.Attestor
}

func newHarness(t *testing.T) *harness {
	t.Helper()
	app := testkeeper.EVMTestApp
	ctx := app.GetContextForDeliverTx([]byte{}).WithBlockHeight(startHeight).WithBlockTime(time.Unix(1_800_000_000, 0))
	ctx = ctx.WithGasMeter(sdk.NewInfiniteGasMeterWithMultiplier(ctx))
	k, ctx := xwebtestutil.NewKeeper(app, ctx)
	k.InitGenesis(ctx, *types.DefaultGenesis())
	h := &harness{t: t, keeper: &k, attestors: xwebtestutil.Attestors(3)}
	for _, attestor := range h.attestors {
		require.NoError(t, k.RegisterAttestor(ctx, types.MsgRegisterAttestor{Authority: authority,
			Attestor: attestor.Registration()}))
	}
	require.NoError(t, k.UpdateParams(ctx, types.MsgSetParams{Authority: authority, Fee: sdk.NewInt(fee),
		MaxPayloadBytes: payloadCap, MaxCallbackGas: callbackCap, TimeoutBlocks: timeout}))
	require.NoError(t, k.Unpause(ctx, types.MsgUnpause{Authority: authority}))
	var err error
	h.precompile, err = xweb.NewPrecompile(keepers{Keepers: app.GetPrecompileKeepers(), xweb: &k})
	require.NoError(t, err)
	h.stateDB = state.NewDBImpl(ctx, &app.EvmKeeper, false)
	blockCtx, err := app.EvmKeeper.GetVMBlockContext(ctx, core.GasPool(math.MaxUint64))
	require.NoError(t, err)
	cfg := evmtypes.DefaultChainConfig().EthereumConfig(app.EvmKeeper.ChainID(ctx))
	contracts := maps.Clone(app.EvmKeeper.CustomPrecompiles(ctx))
	contracts[precompile] = h.precompile
	h.evm = vm.NewEVM(*blockCtx, h.stateDB, cfg, vm.Config{}, contracts)
	for address, code := range map[common.Address][]byte{recorder: recorderCode, reverter: reverterCode, burner: burnerCode} {
		h.stateDB.SetCode(address, code)
		h.fund(address, 10*fee)
	}
	h.fund(stranger, 10*fee)
	return h
}

func (h *harness) ctx() sdk.Context { return h.stateDB.Ctx() }

func (h *harness) account(address common.Address) sdk.AccAddress {
	return testkeeper.EVMTestApp.EvmKeeper.GetPaxAddressOrDefault(h.ctx(), address)
}

func (h *harness) fund(address common.Address, amount int64) {
	h.t.Helper()
	coins := sdk.NewCoins(sdk.NewCoin(sdk.MustGetBaseDenom(), sdk.NewInt(amount)))
	require.NoError(h.t, testkeeper.EVMTestApp.BankKeeper.MintCoins(h.ctx(), "evm", coins))
	require.NoError(h.t, testkeeper.EVMTestApp.BankKeeper.SendCoinsFromModuleToAccount(h.ctx(), "evm", h.account(address), coins))
}

func (h *harness) balance(of sdk.AccAddress) sdk.Int {
	return testkeeper.EVMTestApp.BankKeeper.GetBalance(h.ctx(), of, sdk.MustGetBaseDenom()).Amount
}

func (h *harness) method(name string) abi.Method {
	h.t.Helper()
	m, ok := h.precompile.GetABI().Methods[name]
	require.True(h.t, ok, name)
	return m
}

func (h *harness) input(name string, args ...interface{}) []byte {
	h.t.Helper()
	m := h.method(name)
	packed, err := m.Inputs.Pack(args...)
	require.NoError(h.t, err)
	return append(append([]byte(nil), m.ID...), packed...)
}

// cost is the documented charge taken before a call runs.
func cost(input []byte, base uint64, signatures int) uint64 {
	return base + xweb.GasPerByte*uint64(len(input)-4) + xweb.GasPerSignature*uint64(signatures)
}

// call runs input against the precompile through the EVM and returns the
// decoded outputs, the gas left and the revert reason.
func (h *harness) call(from common.Address, name string, value *big.Int, gas uint64, args ...interface{}) ([]interface{}, uint64, string) {
	h.t.Helper()
	amount := uint256.NewInt(0)
	if value != nil {
		amount = uint256.MustFromBig(value)
	}
	ret, left, err := h.evm.Call(from, precompile, h.input(name, args...), gas, amount)
	if err != nil {
		require.ErrorIs(h.t, err, vm.ErrExecutionReverted)
		reason, unpackErr := abi.UnpackRevert(ret)
		require.NoError(h.t, unpackErr)
		require.NotEmpty(h.t, reason)
		return nil, left, reason
	}
	out, err := h.method(name).Outputs.Unpack(ret)
	require.NoError(h.t, err)
	return out, left, ""
}

func (h *harness) view(name string, args ...interface{}) []interface{} {
	h.t.Helper()
	input := h.input(name, args...)
	ret, left, err := h.evm.StaticCall(stranger, precompile, input, supplied)
	require.NoError(h.t, err, name)
	require.Equal(h.t, supplied-cost(input, xweb.ViewBaseGas, 0), left, name)
	out, err := h.method(name).Outputs.Unpack(ret)
	require.NoError(h.t, err)
	return out
}

func (h *harness) viewReverts(name string, args ...interface{}) string {
	h.t.Helper()
	ret, _, err := h.evm.StaticCall(stranger, precompile, h.input(name, args...), supplied)
	require.ErrorIs(h.t, err, vm.ErrExecutionReverted)
	reason, err := abi.UnpackRevert(ret)
	require.NoError(h.t, err)
	return reason
}

func weiOf(amount int64) *big.Int {
	return new(big.Int).Mul(big.NewInt(amount), state.UhpxToSweiMultiplier)
}

func (h *harness) request(from common.Address, gas uint64) uint64 {
	h.t.Helper()
	input := h.input(xweb.RequestMethod, types.KindFetch, payload, gas)
	out, left, reason := h.call(from, xweb.RequestMethod, weiOf(fee), supplied, types.KindFetch, payload, gas)
	require.Empty(h.t, reason)
	require.Equal(h.t, supplied-cost(input, xweb.WriteBaseGas, 0), left)
	return out[0].(uint64)
}

func (h *harness) signatures(id uint64, resp []byte, contentDigest [32]byte, length uint32) [][]byte {
	h.t.Helper()
	request, found := h.keeper.GetRequest(h.ctx(), id)
	require.True(h.t, found)
	attestation := h.keeper.Attestation(h.ctx(), request, resp, types.Hash32(contentDigest), length)
	return xwebtestutil.Sign(types.Digest(attestation), h.attestors[0], h.attestors[1])
}

// fulfil submits the two-of-three attested answer to id with gas and returns
// the callback outcome, the gas it used and the gas left.
func (h *harness) fulfil(id uint64, gas uint64) (types.CallbackOutcome, uint64, uint64) {
	h.t.Helper()
	signatures := h.signatures(id, response, digest, fullLength)
	input := h.input(xweb.FulfilMethod, id, response, digest, fullLength, signatures)
	out, left, reason := h.call(stranger, xweb.FulfilMethod, nil, gas, id, response, digest, fullLength, signatures)
	require.Empty(h.t, reason)
	outcome, used := types.CallbackOutcome(out[0].(uint8)), out[1].(uint64)
	require.Equal(h.t, gas-cost(input, xweb.WriteBaseGas, 2)-used-xweb.CallbackRecordGas, left)
	return outcome, used, left
}

func (h *harness) logs(event string) []*ethtypes.Log {
	h.t.Helper()
	topic := h.precompile.GetABI().Events[event].ID
	var out []*ethtypes.Log
	for _, log := range h.stateDB.GetAllLogs() {
		if log.Topics[0] == topic {
			require.Equal(h.t, precompile, log.Address)
			out = append(out, log)
		}
	}
	return out
}

func (h *harness) result(id uint64) xweb.ResultView {
	h.t.Helper()
	return *abi.ConvertType(h.view(xweb.GetResultMethod, id)[0], new(xweb.ResultView)).(*xweb.ResultView)
}

func (h *harness) requestView(id uint64) xweb.RequestView {
	h.t.Helper()
	return *abi.ConvertType(h.view(xweb.GetRequestMethod, id)[0], new(xweb.RequestView)).(*xweb.RequestView)
}

func topicOf(value uint64) common.Hash { return common.BigToHash(new(big.Int).SetUint64(value)) }

func TestRequestTakesTheFeeAndEmits(t *testing.T) {
	h := newHarness(t)
	before := h.balance(h.account(recorder))
	moduleBefore := h.balance(h.keeper.ModuleAddress())
	id := h.request(recorder, callbackGas)
	require.Equal(t, uint64(1), id)
	requireAmount(t, before.SubRaw(fee), h.balance(h.account(recorder)))
	requireAmount(t, moduleBefore.AddRaw(fee), h.balance(h.keeper.ModuleAddress()))
	require.True(t, h.balance(h.account(precompile)).IsZero())

	view := h.requestView(id)
	require.Equal(t, id, view.Id)
	require.Equal(t, recorder, view.Requester)
	require.Equal(t, types.KindFetch, view.Kind)
	require.Equal(t, [32]byte(types.Keccak(payload)), view.PayloadHash)
	require.Equal(t, callbackGas, view.CallbackGas)
	require.Zero(t, weiOf(fee).Cmp(view.Fee))
	require.Equal(t, uint64(startHeight), view.Height)
	require.Equal(t, uint64(startHeight)+timeout, view.TimeoutHeight)
	require.Equal(t, uint8(types.StatusPending), view.Status)

	logs := h.logs("XWebRequested")
	require.Len(t, logs, 1)
	require.Equal(t, []common.Hash{logs[0].Topics[0], topicOf(id), common.BytesToHash(recorder.Bytes())}, logs[0].Topics)
	data, err := h.precompile.GetABI().Events["XWebRequested"].Inputs.NonIndexed().Unpack(logs[0].Data)
	require.NoError(t, err)
	require.Equal(t, types.KindFetch, data[0].(uint8))
	require.Equal(t, payload, data[1].([]byte))
	require.Equal(t, callbackGas, data[2].(uint64))
	require.Zero(t, weiOf(fee).Cmp(data[3].(*big.Int)))
	require.Equal(t, uint64(startHeight)+timeout, data[4].(uint64))

	require.Equal(t, uint64(2), h.request(recorder, callbackGas))
}

func TestRequestRefusesWrongPayment(t *testing.T) {
	h := newHarness(t)
	before := h.balance(h.account(recorder))
	_, left, reason := h.call(recorder, xweb.RequestMethod, weiOf(fee-1), supplied, types.KindFetch, payload, callbackGas)
	require.Contains(t, reason, "fee")
	input := h.input(xweb.RequestMethod, types.KindFetch, payload, callbackGas)
	require.Equal(t, supplied-cost(input, xweb.WriteBaseGas, 0), left)

	withRemainder := new(big.Int).Add(weiOf(fee), big.NewInt(1))
	_, _, reason = h.call(recorder, xweb.RequestMethod, withRemainder, supplied, types.KindFetch, payload, callbackGas)
	require.Contains(t, reason, "non-zero wei remainder")

	_, _, reason = h.call(recorder, xweb.RequestMethod, nil, supplied, types.KindFetch, payload, callbackGas)
	require.Contains(t, reason, "fee")

	_, _, reason = h.call(recorder, xweb.RequestMethod, weiOf(fee), supplied, types.KindFetch, payload, callbackCap+1)
	require.Contains(t, reason, "callback gas")

	requireAmount(t, before, h.balance(h.account(recorder)))
	require.Equal(t, uint64(0), h.keeper.Nonce(h.ctx()))
	require.Empty(t, h.logs("XWebRequested"))

	input = h.input(xweb.RequestMethod, types.KindFetch, payload, callbackGas)
	_, left, err := h.evm.StaticCall(recorder, precompile, input, supplied)
	require.ErrorIs(t, err, vm.ErrExecutionReverted)
	require.Equal(t, supplied-cost(input, xweb.WriteBaseGas, 0), left)

	_, left, err = h.evm.Call(recorder, precompile, input, cost(input, xweb.WriteBaseGas, 0)-1, uint256.NewInt(0))
	require.ErrorIs(t, err, vm.ErrOutOfGas)
	require.Zero(t, left)
}

func TestFulfilDeliversTheCallback(t *testing.T) {
	h := newHarness(t)
	id := h.request(recorder, callbackGas)
	payouts := []sdk.Int{h.balance(h.attestors[0].Payout), h.balance(h.attestors[1].Payout), h.balance(h.attestors[2].Payout)}
	moduleBefore := h.balance(h.keeper.ModuleAddress())

	outcome, used, _ := h.fulfil(id, supplied)
	require.Equal(t, types.CallbackDelivered, outcome)
	require.Positive(t, used)
	require.LessOrEqual(t, used, callbackGas)

	require.Equal(t, topicOf(id), h.stateDB.GetState(recorder, common.Hash{}))
	require.Equal(t, common.BytesToHash(precompile.Bytes()), h.stateDB.GetState(recorder, common.BigToHash(big.NewInt(1))))
	require.Equal(t, common.Hash(digest), h.stateDB.GetState(recorder, common.BigToHash(big.NewInt(2))))

	requireAmount(t, payouts[0].AddRaw(500_002), h.balance(h.attestors[0].Payout))
	requireAmount(t, payouts[1].AddRaw(500_001), h.balance(h.attestors[1].Payout))
	requireAmount(t, payouts[2], h.balance(h.attestors[2].Payout))
	requireAmount(t, moduleBefore.SubRaw(fee), h.balance(h.keeper.ModuleAddress()))

	result := h.result(id)
	require.Equal(t, id, result.RequestId)
	require.Equal(t, response, result.Response)
	require.Equal(t, digest, result.ContentDigest)
	require.Equal(t, fullLength, result.FullLength)
	require.Equal(t, []common.Address{common.Address(h.attestors[0].Signer), common.Address(h.attestors[1].Signer)}, result.Signers)
	require.Equal(t, uint64(startHeight), result.Height)
	require.Equal(t, uint8(types.CallbackDelivered), result.Callback)
	require.Equal(t, used, result.CallbackGasUsed)
	require.Equal(t, uint8(types.StatusFulfilled), h.requestView(id).Status)

	logs := h.logs("XWebFulfilled")
	require.Len(t, logs, 1)
	require.Equal(t, []common.Hash{logs[0].Topics[0], topicOf(id), common.BytesToHash(recorder.Bytes())}, logs[0].Topics)
	data, err := h.precompile.GetABI().Events["XWebFulfilled"].Inputs.NonIndexed().Unpack(logs[0].Data)
	require.NoError(t, err)
	require.Equal(t, digest, data[0].([32]byte))
	require.Equal(t, fullLength, data[1].(uint32))
	require.Equal(t, uint8(types.CallbackDelivered), data[2].(uint8))
	require.Equal(t, used, data[3].(uint64))

	signatures := h.signatures(id, response, digest, fullLength)
	_, _, reason := h.call(stranger, xweb.FulfilMethod, nil, supplied, id, response, digest, fullLength, signatures)
	require.Contains(t, reason, "fulfilled")
	_, _, reason = h.call(stranger, xweb.RefundMethod, nil, supplied, id)
	require.Contains(t, reason, "fulfilled")
}

func TestFulfilRecordsARevertedCallback(t *testing.T) {
	h := newHarness(t)
	id := h.request(reverter, callbackGas)
	outcome, used, _ := h.fulfil(id, supplied)
	require.Equal(t, types.CallbackReverted, outcome)
	// PUSH1, PUSH1 and a zero-length REVERT.
	require.Equal(t, uint64(6), used)
	result := h.result(id)
	require.Equal(t, uint8(types.CallbackReverted), result.Callback)
	require.Equal(t, uint64(6), result.CallbackGasUsed)
	require.Equal(t, uint8(types.StatusFulfilled), h.requestView(id).Status)
}

func TestFulfilBoundsTheCallbackByTheModuleMaximum(t *testing.T) {
	h := newHarness(t)
	id := h.request(burner, callbackGas)
	lowered := uint64(50_000)
	require.NoError(t, h.keeper.UpdateParams(h.ctx(), types.MsgSetParams{Authority: authority, Fee: sdk.NewInt(fee),
		MaxPayloadBytes: payloadCap, MaxCallbackGas: lowered, TimeoutBlocks: timeout}))
	outcome, used, _ := h.fulfil(id, supplied)
	require.Equal(t, types.CallbackOutOfGas, outcome)
	require.Equal(t, lowered, used)
	result := h.result(id)
	require.Equal(t, uint8(types.CallbackOutOfGas), result.Callback)
	require.Equal(t, lowered, result.CallbackGasUsed)
}

func TestFulfilRefusesWithoutGasForTheCallback(t *testing.T) {
	h := newHarness(t)
	id := h.request(burner, callbackGas)
	signatures := h.signatures(id, response, digest, fullLength)
	input := h.input(xweb.FulfilMethod, id, response, digest, fullLength, signatures)
	short := cost(input, xweb.WriteBaseGas, 2) + callbackGas + xweb.CallbackRecordGas - 1
	_, left, reason := h.call(stranger, xweb.FulfilMethod, nil, short, id, response, digest, fullLength, signatures)
	require.Contains(t, reason, "needs")
	require.Equal(t, callbackGas+xweb.CallbackRecordGas-1, left)
	require.Equal(t, uint8(types.StatusPending), h.requestView(id).Status)
	require.Empty(t, h.logs("XWebFulfilled"))

	outcome, used, left := h.fulfil(id, short+1)
	require.Equal(t, types.CallbackOutOfGas, outcome)
	require.Equal(t, callbackGas, used)
	require.Zero(t, left)
}

func TestFulfilRefusesBadAttestations(t *testing.T) {
	h := newHarness(t)
	id := h.request(recorder, callbackGas)
	signatures := h.signatures(id, response, digest, fullLength)
	_, _, reason := h.call(stranger, xweb.FulfilMethod, nil, supplied, id, []byte("other"), digest, fullLength, signatures)
	require.Contains(t, reason, "attestor")
	_, _, reason = h.call(stranger, xweb.FulfilMethod, nil, supplied, id, response, digest, fullLength, signatures[:1])
	require.Contains(t, reason, "threshold")
	_, _, reason = h.call(stranger, xweb.FulfilMethod, weiOf(1), supplied, id, response, digest, fullLength, signatures)
	require.Contains(t, reason, "payable")
	_, _, reason = h.call(stranger, xweb.FulfilMethod, nil, supplied, id+1, response, digest, fullLength, signatures)
	require.Contains(t, reason, "not found")
	input := h.input(xweb.FulfilMethod, id, response, digest, fullLength, signatures)
	_, _, err := h.evm.StaticCall(stranger, precompile, input, supplied)
	require.ErrorIs(t, err, vm.ErrExecutionReverted)
	require.Equal(t, uint8(types.StatusPending), h.requestView(id).Status)
	require.Equal(t, common.Hash{}, h.stateDB.GetState(recorder, common.Hash{}))
}

func TestRefundAfterTheTimeout(t *testing.T) {
	h := newHarness(t)
	id := h.request(recorder, callbackGas)
	before := h.balance(h.account(recorder))
	_, _, reason := h.call(stranger, xweb.RefundMethod, nil, supplied, id)
	require.Contains(t, reason, "refundable at height")

	h.stateDB.WithCtx(h.ctx().WithBlockHeight(startHeight + int64(timeout)))
	input := h.input(xweb.RefundMethod, id)
	_, left, reason := h.call(stranger, xweb.RefundMethod, nil, supplied, id)
	require.Empty(t, reason)
	require.Equal(t, supplied-cost(input, xweb.WriteBaseGas, 0), left)
	requireAmount(t, before.AddRaw(fee), h.balance(h.account(recorder)))
	require.Equal(t, uint8(types.StatusRefunded), h.requestView(id).Status)

	logs := h.logs("XWebRefunded")
	require.Len(t, logs, 1)
	require.Equal(t, []common.Hash{logs[0].Topics[0], topicOf(id), common.BytesToHash(recorder.Bytes())}, logs[0].Topics)
	data, err := h.precompile.GetABI().Events["XWebRefunded"].Inputs.NonIndexed().Unpack(logs[0].Data)
	require.NoError(t, err)
	require.Zero(t, weiOf(fee).Cmp(data[0].(*big.Int)))

	_, _, reason = h.call(stranger, xweb.RefundMethod, nil, supplied, id)
	require.Contains(t, reason, "refunded")
	signatures := h.signatures(id, response, digest, fullLength)
	_, _, reason = h.call(stranger, xweb.FulfilMethod, nil, supplied, id, response, digest, fullLength, signatures)
	require.Contains(t, reason, "refunded")
	requireAmount(t, before.AddRaw(fee), h.balance(h.account(recorder)))
}

func TestViews(t *testing.T) {
	h := newHarness(t)
	require.Zero(t, weiOf(fee).Cmp(h.view(xweb.FeeMethod)[0].(*big.Int)))
	require.Equal(t, uint32(2), h.view(xweb.ThresholdMethod)[0].(uint32))

	out := h.view(xweb.GetAttestorsMethod)
	var attestors []xweb.AttestorView
	attestors = *abi.ConvertType(out[0], &attestors).(*[]xweb.AttestorView)
	require.Len(t, attestors, 3)
	for i, attestor := range h.attestors {
		require.Equal(t, common.Address(attestor.Signer), attestors[i].Signer)
		require.Equal(t, attestor.Payout.String(), attestors[i].Payout)
	}
	require.Equal(t, uint32(2), out[1].(uint32))

	params := *abi.ConvertType(h.view(xweb.GetParamsMethod)[0], new(xweb.ParamsView)).(*xweb.ParamsView)
	require.Zero(t, weiOf(fee).Cmp(params.Fee))
	require.Equal(t, payloadCap, params.MaxPayloadBytes)
	require.Equal(t, callbackCap, params.MaxCallbackGas)
	require.Equal(t, timeout, params.TimeoutBlocks)
	require.False(t, params.Paused)

	require.Contains(t, h.viewReverts(xweb.GetRequestMethod, uint64(7)), "not found")
	id := h.request(recorder, callbackGas)
	require.Contains(t, h.viewReverts(xweb.GetResultMethod, id), "no result")

	require.NoError(t, h.keeper.Pause(h.ctx(), types.MsgPause{Authority: authority}))
	params = *abi.ConvertType(h.view(xweb.GetParamsMethod)[0], new(xweb.ParamsView)).(*xweb.ParamsView)
	require.True(t, params.Paused)
	_, _, reason := h.call(recorder, xweb.RequestMethod, weiOf(fee), supplied, types.KindFetch, payload, callbackGas)
	require.Contains(t, reason, "paused")
}

func TestRefusesDelegateCallAndAnUnwiredKeeper(t *testing.T) {
	h := newHarness(t)
	input := h.input(xweb.FeeMethod)
	ret, left, err := h.precompile.RunAndCalculateGas(h.evm, stranger, stranger, input, supplied, big.NewInt(0), nil, false, true)
	require.ErrorIs(t, err, vm.ErrExecutionReverted)
	require.Equal(t, supplied-cost(input, xweb.ViewBaseGas, 0), left)
	reason, err := abi.UnpackRevert(ret)
	require.NoError(t, err)
	require.Contains(t, reason, "delegatecall")

	unwired, err := xweb.NewPrecompile(testkeeper.EVMTestApp.GetPrecompileKeepers())
	require.NoError(t, err)
	ret, _, err = unwired.RunAndCalculateGas(h.evm, stranger, stranger, input, supplied, big.NewInt(0), nil, true, false)
	require.ErrorIs(t, err, vm.ErrExecutionReverted)
	reason, err = abi.UnpackRevert(ret)
	require.NoError(t, err)
	require.Contains(t, reason, "not wired")

	signatures := h.signatures(h.request(recorder, callbackGas), response, digest, fullLength)
	fulfil := h.input(xweb.FulfilMethod, uint64(1), response, digest, fullLength, signatures)
	ret, _, err = unwired.RunAndCalculateGas(h.evm, stranger, stranger, fulfil, supplied, big.NewInt(0), nil, false, false)
	require.ErrorIs(t, err, vm.ErrExecutionReverted)
	reason, err = abi.UnpackRevert(ret)
	require.NoError(t, err)
	require.Contains(t, reason, "not wired")
}

// TestABIMatchesTheSolidityInterface pins abi.json to the signatures the
// Foundry suite checks against the Solidity interface, and the interface the
// contracts import to the precompile's copy.
func TestABIMatchesTheSolidityInterface(t *testing.T) {
	h := newHarness(t)
	suite, err := os.ReadFile("../../contracts/test/XWebConsumer.t.sol")
	require.NoError(t, err)
	parsed := h.precompile.GetABI()
	require.Len(t, parsed.Methods, 9)
	require.Len(t, parsed.Events, 3)
	for _, method := range parsed.Methods {
		require.True(t, strings.Contains(string(suite), `"`+method.Sig+`"`), method.Sig)
	}
	for _, event := range parsed.Events {
		require.True(t, strings.Contains(string(suite), `"`+event.Sig+`"`), event.Sig)
	}
	require.True(t, strings.Contains(string(suite), `"`+xweb.CallbackSignature+`"`))
	require.Equal(t, crypto.Keccak256([]byte(xweb.CallbackSignature))[:4],
		abi.NewMethod(xweb.CallbackMethod, xweb.CallbackMethod, abi.Function, "nonpayable", false, false,
			abi.Arguments{{Type: mustType(t, "uint64")}, {Type: mustType(t, "bytes32")}, {Type: mustType(t, "uint32")},
				{Type: mustType(t, "bytes")}}, nil).ID)

	local, err := os.ReadFile("XWeb.sol")
	require.NoError(t, err)
	imported, err := os.ReadFile("../../contracts/src/precompiles/IXWeb.sol")
	require.NoError(t, err)
	require.True(t, bytes.Equal(local, imported))
}

func mustType(t *testing.T, name string) abi.Type {
	t.Helper()
	typ, err := abi.NewType(name, "", nil)
	require.NoError(t, err)
	return typ
}

func requireAmount(t *testing.T, want, got sdk.Int) {
	t.Helper()
	require.True(t, want.Equal(got), "want %s, got %s", want, got)
}
