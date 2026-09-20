package layerxanchor_test

import (
	"math/big"
	"testing"
	"time"

	"github.com/ethereum/go-ethereum/accounts/abi"
	"github.com/ethereum/go-ethereum/common"
	ethtypes "github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/core/vm"
	gigaprecompiles "github.com/sidiora-labs/paxeer-network/engine/executor/precompiles"
	"github.com/sidiora-labs/paxeer-network/layerxproof/testvectors"
	evmkeeper "github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/evm/state"
	anchorkeeper "github.com/sidiora-labs/paxeer-network/modules/layerxanchor/keeper"
	anchortypes "github.com/sidiora-labs/paxeer-network/modules/layerxanchor/types"
	app "github.com/sidiora-labs/paxeer-network/node"
	pcommon "github.com/sidiora-labs/paxeer-network/precompiles/common"
	"github.com/sidiora-labs/paxeer-network/precompiles/layerxanchor"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/stretchr/testify/require"
)

const (
	bond           = int64(5_000_000)
	unbondingDelay = uint64(1000)
)

var weiPerUnit = big.NewInt(1_000_000_000_000)

type party struct {
	account sdk.AccAddress
	address common.Address
}

type harness struct {
	t          *testing.T
	app        *app.App
	stateDB    *state.DBImpl
	evm        *vm.EVM
	precompile *pcommon.Precompile
	anchor     anchorkeeper.Keeper
	fixture    testvectors.Fixture
	authority  party
	reporter   party
	operators  []party
	ids        [][32]byte
	signers    []common.Address
	reason     string
}

func (h *harness) ctx() sdk.Context { return h.stateDB.Ctx() }

func (h *harness) party(tag byte, associated bool) party {
	h.t.Helper()
	p := party{account: make(sdk.AccAddress, 20)}
	for i := range p.account {
		p.account[i] = tag
	}
	if associated {
		for i := range p.address {
			p.address[i] = tag ^ 0xff
		}
		h.app.EvmKeeper.SetAddressMapping(h.ctx(), p.account, p.address)
	} else {
		p.address = common.BytesToAddress(p.account)
	}
	return p
}

func (h *harness) fund(to sdk.AccAddress, amount int64) {
	h.t.Helper()
	coins := sdk.NewCoins(sdk.NewCoin("uhpx", sdk.NewInt(amount)))
	require.NoError(h.t, h.app.BankKeeper.MintCoins(h.ctx(), "evm", coins))
	require.NoError(h.t, h.app.BankKeeper.SendCoinsFromModuleToAccount(h.ctx(), "evm", to, coins))
}

func (h *harness) balance(of sdk.AccAddress) int64 {
	return h.app.BankKeeper.GetBalance(h.ctx(), of, "uhpx").Amount.Int64()
}

func newHarness(t *testing.T, mutate func(*anchortypes.Params)) *harness {
	t.Helper()
	testApp := app.Setup(t, false, true, false)
	ctx := testApp.GetContextForDeliverTx([]byte{}).WithBlockHeight(8).WithBlockTime(time.Unix(1_800_000_000, 0))
	h := &harness{t: t, app: testApp, anchor: testApp.LayerXAnchorKeeper}
	h.stateDB = state.NewDBImpl(ctx, &testApp.EvmKeeper, true)
	h.evm = &vm.EVM{StateDB: h.stateDB}
	p, err := layerxanchor.NewPrecompile(testApp.GetPrecompileKeepers())
	require.NoError(t, err)
	h.precompile = p
	h.fixture, err = testvectors.LoadAnchor()
	require.NoError(t, err)
	h.authority, h.reporter = h.party(0xa1, true), h.party(0xa2, false)

	context := h.fixture["context"][0]
	params := anchortypes.DefaultParams(h.authority.account.String())
	params.PaxeerChainID, err = context.Uint64("paxeer_chain_id")
	require.NoError(t, err)
	network, err := context.Uint64("network_id")
	require.NoError(t, err)
	params.NetworkID = uint32(network)
	require.Equal(t, common.HexToAddress(layerxanchor.LayerXAnchorAddress), common.Address(params.SettlementContract))
	params.Threshold = 2
	params.UnbondingDelaySeconds = unbondingDelay
	if mutate != nil {
		mutate(&params)
	}
	require.NoError(t, h.anchor.SetParams(h.ctx(), params))

	for index, guarantor := range h.fixture["guarantors"][:3] {
		id, err := guarantor.Array32("guarantor_id")
		require.NoError(t, err)
		raw, err := guarantor.Bytes("signer")
		require.NoError(t, err)
		operator := h.party(byte(0xb0+index), true)
		h.fund(operator.account, 2*bond)
		h.ids, h.signers, h.operators = append(h.ids, id), append(h.signers, common.BytesToAddress(raw)), append(h.operators, operator)
	}
	return h
}

func (h *harness) method(name string) *abi.Method {
	h.t.Helper()
	m, ok := h.precompile.GetABI().Methods[name]
	require.True(h.t, ok, name)
	return &m
}

func (h *harness) input(name string, args ...interface{}) []byte {
	h.t.Helper()
	m := h.method(name)
	packed, err := m.Inputs.Pack(args...)
	require.NoError(h.t, err)
	return append(append([]byte(nil), m.ID...), packed...)
}

// run enters through the EVM precompile entry point as the interpreter does:
// the call value reaches the precompile address first, and a revert restores
// the snapshot taken before the call.
func (h *harness) run(from party, units int64, readOnly bool, name string, args ...interface{}) ([]interface{}, error) {
	h.t.Helper()
	snapshot := h.stateDB.Snapshot()
	var value *big.Int
	if units != 0 {
		value = new(big.Int).Mul(big.NewInt(units), weiPerUnit)
		precompileAccount := h.app.EvmKeeper.GetPaxAddressOrDefault(h.ctx(), h.precompile.Address())
		require.NoError(h.t, h.app.BankKeeper.SendCoins(h.ctx(), from.account, precompileAccount,
			sdk.NewCoins(sdk.NewCoin("uhpx", sdk.NewInt(units)))))
	}
	res, err := h.precompile.Run(h.evm, from.address, from.address, h.input(name, args...), value, readOnly, false, nil)
	if err != nil {
		require.ErrorIs(h.t, err, vm.ErrExecutionReverted)
		reason, unpackErr := abi.UnpackRevert(res)
		require.NoError(h.t, unpackErr)
		require.NotEmpty(h.t, reason)
		h.reason = reason
		h.stateDB.RevertToSnapshot(snapshot)
		return nil, err
	}
	out, err := h.method(name).Outputs.Unpack(res)
	require.NoError(h.t, err)
	return out, nil
}

func (h *harness) call(from party, name string, args ...interface{}) ([]interface{}, error) {
	return h.run(from, 0, false, name, args...)
}

func (h *harness) view(name string, args ...interface{}) []interface{} {
	h.t.Helper()
	out, err := h.run(h.reporter, 0, true, name, args...)
	require.NoError(h.t, err)
	return out
}

func (h *harness) logs(event string) []*ethtypes.Log {
	h.t.Helper()
	id := h.precompile.GetABI().Events[event].ID
	var out []*ethtypes.Log
	for _, log := range h.stateDB.GetAllLogs() {
		if log.Topics[0] == id {
			require.Equal(h.t, h.precompile.Address(), log.Address)
			out = append(out, log)
		}
	}
	return out
}

func (h *harness) invariant() {
	h.t.Helper()
	message, broken := anchorkeeper.BalanceInvariant(h.anchor)(h.ctx())
	require.False(h.t, broken, message)
}

func (h *harness) vector(section, name string) testvectors.Vector {
	h.t.Helper()
	for _, v := range h.fixture[section] {
		if v.Name == name {
			return v
		}
	}
	h.t.Fatalf("no vector %s/%s", section, name)
	return testvectors.Vector{}
}

func (h *harness) checkpointArgs(name string) []interface{} {
	h.t.Helper()
	v := h.vector("checkpoints", name)
	header, err := v.Bytes("header")
	require.NoError(h.t, err)
	signature, err := v.Bytes("header_signature")
	require.NoError(h.t, err)
	certificate, err := v.Bytes("certificate")
	require.NoError(h.t, err)
	return []interface{}{header, signature, certificate}
}

func (h *harness) submit(name string) ([]interface{}, error) {
	return h.call(h.reporter, layerxanchor.SubmitCheckpointMethod, h.checkpointArgs(name)...)
}

func (h *harness) attestation(name string) []byte {
	h.t.Helper()
	raw, err := h.vector("attestations", name).Bytes("attestation")
	require.NoError(h.t, err)
	return raw
}

func (h *harness) advance(seconds uint64) {
	ctx := h.ctx()
	h.stateDB.WithCtx(ctx.WithBlockTime(ctx.BlockTime().Add(time.Duration(seconds) * time.Second)).WithBlockHeight(ctx.BlockHeight() + 1))
}

// bootstrap bonds and activates the three guarantors and authorizes the
// sequencer, all through the precompile.
func (h *harness) bootstrap() {
	h.t.Helper()
	for index := range h.ids {
		_, err := h.run(h.operators[index], bond, false, layerxanchor.RegisterGuarantorMethod, h.ids[index], h.signers[index])
		require.NoError(h.t, err)
		_, err = h.call(h.operators[index], layerxanchor.ActivateGuarantorMethod, h.ids[index])
		require.Error(h.t, err)
		_, err = h.call(h.authority, layerxanchor.ActivateGuarantorMethod, h.ids[index])
		require.NoError(h.t, err)
	}
	context := h.fixture["context"][0]
	sequencerID, err := context.Array32("sequencer_id")
	require.NoError(h.t, err)
	sequencerKey, err := context.Array32("sequencer_public_key")
	require.NoError(h.t, err)
	_, err = h.call(h.reporter, layerxanchor.SetSequencerAuthorizationMethod, sequencerID, sequencerKey, uint64(1), uint64(8))
	require.Error(h.t, err)
	_, err = h.call(h.authority, layerxanchor.SetSequencerAuthorizationMethod, sequencerID, sequencerKey, uint64(1), uint64(8))
	require.NoError(h.t, err)
	require.Len(h.t, h.logs("GuarantorRegistered"), 3)
	require.Len(h.t, h.logs("GuarantorActivated"), 3)
	require.Len(h.t, h.logs("SequencerAuthorized"), 1)
	h.invariant()
}

func TestQuorumCheckpointFinalizesThroughRun(t *testing.T) {
	h := newHarness(t, nil)
	h.bootstrap()
	require.Equal(t, []interface{}{uint64(0), false}, h.view(layerxanchor.LatestFinalizedMethod))
	require.Equal(t, uint32(2), h.view(layerxanchor.ThresholdMethod)[0])

	v := h.vector("checkpoints", "batch_1_quorum")
	expectedID, _ := v.Array32("checkpoint_id")
	expectedState, _ := v.Array32("resulting_state_root")
	expectedReceipts, _ := v.Array32("receipt_root")
	out, err := h.submit("batch_1_quorum")
	require.NoError(t, err)
	require.Equal(t, []interface{}{expectedID, anchortypes.CheckpointFinal}, out)

	submitted, finalized := h.logs("CheckpointSubmitted"), h.logs("CheckpointFinalized")
	require.Len(t, submitted, 1)
	require.Len(t, finalized, 1)
	for _, log := range []*ethtypes.Log{submitted[0], finalized[0]} {
		require.Equal(t, common.BigToHash(big.NewInt(1)), log.Topics[1])
		require.Equal(t, common.Hash(expectedID), log.Topics[2])
		require.Equal(t, expectedState[:], log.Data[:32])
		require.Equal(t, expectedReceipts[:], log.Data[32:64])
	}

	require.Equal(t, []interface{}{uint64(1), true}, h.view(layerxanchor.LatestFinalizedMethod))
	require.Equal(t, []interface{}{expectedState, true}, h.view(layerxanchor.FinalizedStateRootMethod, uint64(1)))
	require.Equal(t, []interface{}{expectedReceipts, true}, h.view(layerxanchor.FinalizedReceiptRootMethod, uint64(1)))
	require.Equal(t, []interface{}{[32]byte{}, false}, h.view(layerxanchor.FinalizedStateRootMethod, uint64(2)))
	require.Equal(t, anchortypes.CheckpointFinal, h.view(layerxanchor.StatusOfMethod, uint64(1))[0])
	require.Equal(t, anchortypes.CheckpointUnknown, h.view(layerxanchor.StatusOfMethod, uint64(2))[0])

	var checkpoint struct{ Value layerxanchor.CheckpointView }
	checkpoint.Value = *abi.ConvertType(h.view(layerxanchor.CheckpointMethod, uint64(1))[0], new(layerxanchor.CheckpointView)).(*layerxanchor.CheckpointView)
	require.Equal(t, expectedID, checkpoint.Value.CheckpointId)
	require.Equal(t, expectedState, checkpoint.Value.StateRoot)
	require.Equal(t, uint64(10), checkpoint.Value.LastSequence)
	require.Equal(t, uint8(3), checkpoint.Value.Signers)
	require.Equal(t, uint8(0x1f), checkpoint.Value.AvailabilityMask)
	require.Equal(t, anchortypes.CheckpointFinal, checkpoint.Value.Status)

	var guarantor struct{ Value layerxanchor.GuarantorView }
	guarantor.Value = *abi.ConvertType(h.view(layerxanchor.GuarantorMethod, h.ids[0])[0], new(layerxanchor.GuarantorView)).(*layerxanchor.GuarantorView)
	require.Equal(t, h.signers[0], guarantor.Value.Signer)
	require.Equal(t, h.operators[0].address, guarantor.Value.Operator)
	require.Equal(t, big.NewInt(bond), guarantor.Value.Bond)
	require.Equal(t, anchortypes.GuarantorActive, guarantor.Value.Status)
	require.True(t, guarantor.Value.Eligible)

	_, err = h.submit("batch_1_quorum")
	require.Error(t, err)
	for _, name := range []string{"batch_2_sequence_gap", "batch_2_wrong_previous_root"} {
		_, err = h.submit(name)
		require.Error(t, err, name)
	}
	out, err = h.submit("batch_2_quorum")
	require.NoError(t, err)
	require.Equal(t, anchortypes.CheckpointFinal, out[1])
	require.Equal(t, []interface{}{uint64(2), true}, h.view(layerxanchor.LatestFinalizedMethod))
	h.invariant()
}

func TestBelowThresholdStaysSubmittedThroughRun(t *testing.T) {
	h := newHarness(t, nil)
	h.bootstrap()
	out, err := h.submit("batch_1_below_threshold")
	require.NoError(t, err)
	require.Equal(t, anchortypes.CheckpointSubmitted, out[1])
	require.Empty(t, h.logs("CheckpointFinalized"))
	require.Equal(t, anchortypes.CheckpointSubmitted, h.view(layerxanchor.StatusOfMethod, uint64(1))[0])
	require.Equal(t, []interface{}{[32]byte{}, false}, h.view(layerxanchor.FinalizedStateRootMethod, uint64(1)))
	require.Equal(t, []interface{}{[32]byte{}, false}, h.view(layerxanchor.FinalizedReceiptRootMethod, uint64(1)))
	require.Equal(t, []interface{}{uint64(0), false}, h.view(layerxanchor.LatestFinalizedMethod))
	_, err = h.call(h.reporter, layerxanchor.FinalizeMethod, uint64(1))
	require.Error(t, err)

	out, err = h.call(h.reporter, layerxanchor.SubmitAvailabilityMethod, h.attestation("batch_1_attestation_1"))
	require.NoError(t, err)
	require.Equal(t, uint8(0x1f), out[0])
	require.Len(t, h.logs("AvailabilityAttested"), 1)
	_, err = h.call(h.reporter, layerxanchor.SubmitAvailabilityMethod, h.attestation("batch_1_attestation_1"))
	require.Error(t, err)
	_, err = h.call(h.reporter, layerxanchor.SubmitAvailabilityMethod, h.attestation("batch_1_conflicting_attestation_2"))
	require.Error(t, err)
	require.Equal(t, anchortypes.CheckpointSubmitted, h.view(layerxanchor.StatusOfMethod, uint64(1))[0])
}

func TestRefusedCheckpointsRevert(t *testing.T) {
	h := newHarness(t, nil)
	h.bootstrap()
	refused := 0
	for _, v := range h.fixture["checkpoints"] {
		if v.Fields["refusal"] == "" {
			continue
		}
		refused++
		_, err := h.submit(v.Name)
		require.Error(t, err, v.Name)
		batch, _ := v.Uint64("batch_number")
		require.Equal(t, anchortypes.CheckpointUnknown, h.view(layerxanchor.StatusOfMethod, batch)[0], v.Name)
	}
	require.Equal(t, 9, refused)
	require.Empty(t, h.anchor.GetCheckpoints(h.ctx()))
	require.Empty(t, h.logs("CheckpointSubmitted"))

	args := h.checkpointArgs("batch_1_quorum")
	_, err := h.call(h.reporter, layerxanchor.SubmitCheckpointMethod, args[0], args[1].([]byte)[:63], args[2])
	require.Error(t, err)
}

func TestEquivocationSlashesOnceThroughRun(t *testing.T) {
	h := newHarness(t, nil)
	h.bootstrap()
	first, second := h.attestation("batch_1_attestation_0"), h.attestation("batch_1_conflicting_attestation_0")
	_, err := h.call(h.reporter, layerxanchor.SubmitEquivocationMethod, first, first)
	require.Error(t, err)

	out, err := h.call(h.reporter, layerxanchor.SubmitEquivocationMethod, first, second)
	require.NoError(t, err)
	require.Equal(t, big.NewInt(bond), out[0])
	require.Equal(t, bond/10, h.balance(h.reporter.account))
	slashed := h.logs("GuarantorSlashed")
	require.Len(t, slashed, 1)
	require.Equal(t, common.Hash(h.ids[0]), slashed[0].Topics[1])
	h.invariant()

	for _, pair := range [][2][]byte{{first, second}, {second, first}} {
		_, err = h.call(h.reporter, layerxanchor.SubmitEquivocationMethod, pair[0], pair[1])
		require.Error(t, err)
	}
	require.Len(t, h.logs("GuarantorSlashed"), 1)
	require.Len(t, h.anchor.GetSlashRecords(h.ctx()), 1)
	require.Equal(t, bond/10, h.balance(h.reporter.account))
	guarantor, _ := h.anchor.GetGuarantor(h.ctx(), h.ids[0])
	require.Equal(t, anchortypes.GuarantorEjected, guarantor.Status)
	h.invariant()

	_, err = h.submit("batch_1_quorum")
	require.Error(t, err)
}

func TestUnbondingThroughRun(t *testing.T) {
	h := newHarness(t, nil)
	h.bootstrap()
	half := big.NewInt(bond / 2)

	_, err := h.run(h.operators[0], bond, false, layerxanchor.IncreaseBondMethod, h.ids[1])
	require.Error(t, err)
	_, err = h.run(h.operators[2], bond, false, layerxanchor.IncreaseBondMethod, h.ids[2])
	require.NoError(t, err)
	require.Len(t, h.logs("BondIncreased"), 1)
	require.Equal(t, int64(0), h.balance(h.operators[2].account))
	h.invariant()

	_, err = h.call(h.operators[1], layerxanchor.BeginUnbondMethod, h.ids[0], half)
	require.Error(t, err)
	_, err = h.call(h.reporter, layerxanchor.BeginUnbondMethod, h.ids[0], half)
	require.Error(t, err)
	for index := 0; index < 2; index++ {
		out, err := h.call(h.operators[index], layerxanchor.BeginUnbondMethod, h.ids[index], half)
		require.NoError(t, err)
		require.Equal(t, uint64(h.ctx().BlockTime().Unix())+unbondingDelay, out[0])
	}
	require.Len(t, h.logs("UnbondBegun"), 2)
	h.invariant()

	_, err = h.call(h.operators[0], layerxanchor.CompleteUnbondMethod, h.ids[0])
	require.Error(t, err)
	h.advance(unbondingDelay - 1)
	_, err = h.call(h.operators[0], layerxanchor.CompleteUnbondMethod, h.ids[0])
	require.Error(t, err)
	require.Equal(t, bond, h.balance(h.operators[0].account))

	// Inside the delay the unbonding half is slashed together with the bond.
	out, err := h.call(h.reporter, layerxanchor.SubmitEquivocationMethod, h.attestation("batch_1_attestation_0"),
		h.attestation("batch_1_conflicting_attestation_0"))
	require.NoError(t, err)
	require.Equal(t, big.NewInt(bond), out[0])
	h.invariant()

	h.advance(1)
	_, err = h.call(h.operators[0], layerxanchor.CompleteUnbondMethod, h.ids[0])
	require.Error(t, err)
	out, err = h.call(h.operators[1], layerxanchor.CompleteUnbondMethod, h.ids[1])
	require.NoError(t, err)
	require.Equal(t, half, out[0])
	require.Equal(t, bond+bond/2, h.balance(h.operators[1].account))
	require.Len(t, h.logs("UnbondCompleted"), 1)
	h.invariant()
}

func TestChallengeResolutionIsAuthorityOnlyThroughRun(t *testing.T) {
	h := newHarness(t, func(p *anchortypes.Params) { p.ChallengeWindowSeconds = 50 })
	h.bootstrap()
	challenger := h.party(0xc1, true)
	h.fund(challenger.account, 3_000_000)
	evidence := [32]byte{1}

	out, err := h.submit("batch_1_quorum")
	require.NoError(t, err)
	require.Equal(t, anchortypes.CheckpointSubmitted, out[1])
	_, err = h.run(challenger, 999_999, false, layerxanchor.OpenChallengeMethod, uint64(1), anchortypes.ChallengeFraud, evidence)
	require.Error(t, err)
	require.Equal(t, int64(3_000_000), h.balance(challenger.account))
	_, err = h.run(h.reporter, 0, false, layerxanchor.OpenChallengeMethod, uint64(1), anchortypes.ChallengeFraud, evidence)
	require.Error(t, err)

	out, err = h.run(challenger, 1_000_000, false, layerxanchor.OpenChallengeMethod, uint64(1), anchortypes.ChallengeFraud, evidence)
	require.NoError(t, err)
	challengeID := out[0].(uint64)
	require.Len(t, h.logs("ChallengeOpened"), 1)
	require.Equal(t, int64(2_000_000), h.balance(challenger.account))
	h.invariant()

	h.advance(60)
	_, err = h.call(h.reporter, layerxanchor.FinalizeMethod, uint64(1))
	require.Error(t, err)
	for _, stranger := range []party{challenger, h.reporter, h.operators[0]} {
		_, err = h.call(stranger, layerxanchor.ResolveChallengeMethod, challengeID, true)
		require.Error(t, err)
	}
	require.Empty(t, h.logs("ChallengeResolved"))
	require.Equal(t, anchortypes.CheckpointSubmitted, h.view(layerxanchor.StatusOfMethod, uint64(1))[0])

	_, err = h.call(h.authority, layerxanchor.ResolveChallengeMethod, challengeID, false)
	require.NoError(t, err)
	require.Len(t, h.logs("ChallengeResolved"), 1)
	require.Equal(t, int64(2_000_000), h.balance(challenger.account))
	h.invariant()
	_, err = h.call(h.authority, layerxanchor.ResolveChallengeMethod, challengeID, false)
	require.Error(t, err)

	out, err = h.call(h.reporter, layerxanchor.FinalizeMethod, uint64(1))
	require.NoError(t, err)
	require.Equal(t, true, out[0])
	require.Len(t, h.logs("CheckpointFinalized"), 1)
	require.Equal(t, anchortypes.CheckpointFinal, h.view(layerxanchor.StatusOfMethod, uint64(1))[0])
}

func TestUpheldChallengeSlashesThroughRun(t *testing.T) {
	h := newHarness(t, func(p *anchortypes.Params) { p.ChallengeWindowSeconds = 50 })
	h.bootstrap()
	challenger := h.party(0xc1, true)
	h.fund(challenger.account, 1_000_000)
	_, err := h.submit("batch_1_quorum")
	require.NoError(t, err)
	out, err := h.run(challenger, 1_000_000, false, layerxanchor.OpenChallengeMethod, uint64(1), anchortypes.ChallengeFraud, [32]byte{7})
	require.NoError(t, err)
	_, err = h.call(h.authority, layerxanchor.ResolveChallengeMethod, out[0], true)
	require.NoError(t, err)
	require.Len(t, h.logs("GuarantorSlashed"), 3)
	require.Equal(t, 1_000_000+3*bond/10, h.balance(challenger.account))
	require.Equal(t, anchortypes.CheckpointUnknown, h.view(layerxanchor.StatusOfMethod, uint64(1))[0])
	h.invariant()
}

// The bring-up names the authority in genesis as the cast of the deployer's EVM
// address. The deployer's first signed transaction associates that address with
// its public-key account; the authority must survive that.
func TestCastAuthoritySurvivesAssociation(t *testing.T) {
	var deployer, stranger party
	h := newHarness(t, func(p *anchortypes.Params) {
		for i := range deployer.address {
			deployer.address[i], stranger.address[i] = 0xd1, 0xd2
		}
		p.Authority = sdk.AccAddress(deployer.address[:]).String()
	})
	deployer.account, stranger.account = sdk.AccAddress(deployer.address[:]), sdk.AccAddress(stranger.address[:])
	operator := h.operators[0]

	_, err := h.run(operator, bond, false, layerxanchor.RegisterGuarantorMethod, h.ids[0], h.signers[0])
	require.NoError(t, err)
	require.Equal(t, bond, h.balance(operator.account))
	status := func(id [32]byte) *layerxanchor.GuarantorView {
		return abi.ConvertType(h.view(layerxanchor.GuarantorMethod, id)[0], new(layerxanchor.GuarantorView)).(*layerxanchor.GuarantorView)
	}
	require.Equal(t, anchortypes.GuarantorPending, status(h.ids[0]).Status)

	associated := make(sdk.AccAddress, 20)
	for i := range associated {
		associated[i] = 0xd3
	}
	h.app.EvmKeeper.SetAddressMapping(h.ctx(), associated, deployer.address)
	account, ok := h.app.EvmKeeper.GetPaxAddress(h.ctx(), deployer.address)
	require.True(t, ok)
	require.NotEqual(t, deployer.account, account)

	_, err = h.call(stranger, layerxanchor.ActivateGuarantorMethod, h.ids[0])
	require.Error(t, err)
	_, err = h.call(operator, layerxanchor.ActivateGuarantorMethod, h.ids[0])
	require.Error(t, err)
	require.Equal(t, anchortypes.GuarantorPending, status(h.ids[0]).Status)
	_, err = h.call(deployer, layerxanchor.ActivateGuarantorMethod, h.ids[0])
	require.NoError(t, err)
	require.Equal(t, anchortypes.GuarantorActive, status(h.ids[0]).Status)
	require.True(t, status(h.ids[0]).Eligible)
	require.Equal(t, big.NewInt(bond), status(h.ids[0]).Bond)
	sequencer := [32]byte{7}
	_, err = h.call(stranger, layerxanchor.SetSequencerAuthorizationMethod, sequencer, [32]byte{8}, uint64(1), uint64(8))
	require.Error(t, err)
	_, err = h.call(deployer, layerxanchor.SetSequencerAuthorizationMethod, sequencer, [32]byte{8}, uint64(1), uint64(8))
	require.NoError(t, err)
	h.invariant()
}

func TestCallDiscipline(t *testing.T) {
	h := newHarness(t, nil)
	h.bootstrap()
	// State changes are refused from staticcall.
	_, err := h.run(h.reporter, 0, true, layerxanchor.SubmitCheckpointMethod, h.checkpointArgs("batch_1_quorum")...)
	require.Error(t, err)
	require.Equal(t, anchortypes.CheckpointUnknown, h.view(layerxanchor.StatusOfMethod, uint64(1))[0])
	// Value is refused by every non-payable method.
	h.fund(h.reporter.account, 5)
	_, err = h.run(h.reporter, 1, false, layerxanchor.SubmitCheckpointMethod, h.checkpointArgs("batch_1_quorum")...)
	require.Error(t, err)
	_, err = h.run(h.reporter, 1, false, layerxanchor.ThresholdMethod)
	require.Error(t, err)
	// Delegatecall is refused.
	_, err = h.precompile.Run(h.evm, h.reporter.address, h.reporter.address, h.input(layerxanchor.ThresholdMethod), nil, true, true, nil)
	require.ErrorIs(t, err, vm.ErrExecutionReverted)
	// A bond below the minimum and an unassociated operator are refused.
	stranger := h.party(0xd1, true)
	h.fund(stranger.account, bond)
	_, err = h.run(stranger, 999_999, false, layerxanchor.RegisterGuarantorMethod, [32]byte{0xee}, common.Address{0xee})
	require.Error(t, err)
	require.Equal(t, bond, h.balance(stranger.account))
	h.fund(h.reporter.account, bond)
	_, err = h.run(h.reporter, bond, false, layerxanchor.RegisterGuarantorMethod, [32]byte{0xee}, common.Address{0xee})
	require.Error(t, err)
	_, err = h.run(stranger, bond, false, layerxanchor.RegisterGuarantorMethod, h.ids[0], common.Address{0xee})
	require.Error(t, err)
	_, err = h.run(stranger, bond, false, layerxanchor.RegisterGuarantorMethod, [32]byte{0xee}, h.signers[0])
	require.Error(t, err)
	h.invariant()
}

func TestGasFormula(t *testing.T) {
	h := newHarness(t, nil)
	input := h.input(layerxanchor.SubmitCheckpointMethod, h.checkpointArgs("batch_1_quorum")...)
	body := uint64(len(input) - 4)
	require.Equal(t, layerxanchor.WriteBaseGas+layerxanchor.GasPerByte*body+layerxanchor.GasPerSignature*4, h.precompile.RequiredGas(input))
	require.Equal(t, layerxanchor.Gas(false, body, 4, 0), h.precompile.RequiredGas(input))

	single := h.input(layerxanchor.SubmitCheckpointMethod, h.checkpointArgs("batch_1_below_threshold")...)
	require.Equal(t, layerxanchor.Gas(false, uint64(len(single)-4), 2, 0), h.precompile.RequiredGas(single))

	malformed := h.input(layerxanchor.SubmitCheckpointMethod, []byte{1}, []byte{2}, []byte{0, 1})
	require.Equal(t, layerxanchor.Gas(false, uint64(len(malformed)-4), 33, 0), h.precompile.RequiredGas(malformed))

	attestations, nodes := layerxanchor.CertificateWork(append(append(make([]byte, 360), 0, 0, 0, 64), make([]byte, 64+1)...))
	require.Equal(t, uint64(0), attestations)
	require.Equal(t, uint64(2), nodes)

	equivocation := h.input(layerxanchor.SubmitEquivocationMethod, h.attestation("batch_1_attestation_0"), h.attestation("batch_1_conflicting_attestation_0"))
	require.Equal(t, layerxanchor.Gas(false, uint64(len(equivocation)-4), 2, 0), h.precompile.RequiredGas(equivocation))
	availability := h.input(layerxanchor.SubmitAvailabilityMethod, h.attestation("batch_1_attestation_0"))
	require.Equal(t, layerxanchor.Gas(false, uint64(len(availability)-4), 1, 0), h.precompile.RequiredGas(availability))
	view := h.input(layerxanchor.StatusOfMethod, uint64(1))
	require.Equal(t, layerxanchor.ViewBaseGas+layerxanchor.GasPerByte*32, h.precompile.RequiredGas(view))
	require.Equal(t, h.precompile.RequiredGas(input), h.precompile.RequiredGas(input))
}

func TestWiring(t *testing.T) {
	address := common.HexToAddress(layerxanchor.LayerXAnchorAddress)
	require.Equal(t, common.HexToAddress("0x0000000000000000000000000000000000001014"), address)
	require.True(t, evmkeeper.IsPayablePrecompile(&address))
	require.Contains(t, gigaprecompiles.AllCustomPrecompilesFailFast, address)
}

func TestRevertCarriesReason(t *testing.T) {
	h := newHarness(t, nil)
	_, err := h.call(h.reporter, layerxanchor.ActivateGuarantorMethod, h.ids[0])
	require.ErrorIs(t, err, vm.ErrExecutionReverted)
	require.Contains(t, h.reason, anchortypes.ErrUnauthorized.Error())

	h.fund(h.reporter.account, 5)
	_, err = h.run(h.reporter, 1, false, layerxanchor.ThresholdMethod)
	require.ErrorIs(t, err, vm.ErrExecutionReverted)
	require.NotEmpty(t, h.reason)

	res, err := h.precompile.Run(h.evm, h.reporter.address, h.reporter.address, []byte{0xde, 0xad, 0xbe, 0xef}, nil, false, false, nil)
	require.ErrorIs(t, err, vm.ErrExecutionReverted)
	reason, err := abi.UnpackRevert(res)
	require.NoError(t, err)
	require.NotEmpty(t, reason)
}
