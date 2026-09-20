package layerxverify_test

import (
	"math/big"
	"testing"

	"github.com/ethereum/go-ethereum/accounts/abi"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/vm"
	"github.com/sidiora-labs/paxeer-network/layerxproof/testvectors"
	"github.com/sidiora-labs/paxeer-network/modules/evm/state"
	pcommon "github.com/sidiora-labs/paxeer-network/precompiles/common"
	"github.com/sidiora-labs/paxeer-network/precompiles/layerxverify"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/stretchr/testify/require"
)

type harness struct {
	t          *testing.T
	evm        *vm.EVM
	precompile *pcommon.Precompile
	fixture    testvectors.Fixture
}

func newHarness(t *testing.T) *harness {
	t.Helper()
	stateDB := &state.DBImpl{}
	stateDB.WithCtx(sdk.Context{})
	p, err := layerxverify.NewPrecompile(nil)
	require.NoError(t, err)
	fixture, err := testvectors.Load()
	require.NoError(t, err)
	return &harness{t: t, evm: &vm.EVM{StateDB: stateDB}, precompile: p, fixture: fixture}
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

// call goes through the EVM precompile entry point, as the interpreter does.
func (h *harness) call(name string, args ...interface{}) ([]interface{}, error) {
	h.t.Helper()
	res, err := h.precompile.Run(h.evm, common.Address{}, common.Address{}, h.input(name, args...), nil, true, false, nil)
	if err != nil {
		require.ErrorIs(h.t, err, vm.ErrExecutionReverted)
		require.Nil(h.t, res)
		return nil, err
	}
	out, err := h.method(name).Outputs.Unpack(res)
	require.NoError(h.t, err)
	return out, nil
}

func (h *harness) cases(section string) []testvectors.Vector {
	h.t.Helper()
	cases := h.fixture[section]
	require.NotEmpty(h.t, cases, section)
	return cases
}

func bytesOf(t *testing.T, v testvectors.Vector, key string) []byte {
	t.Helper()
	raw, err := v.Bytes(key)
	require.NoError(t, err)
	return raw
}

func array(t *testing.T, v testvectors.Vector, key string) [32]byte {
	t.Helper()
	out, err := v.Array32(key)
	require.NoError(t, err)
	return out
}

func number(t *testing.T, v testvectors.Vector, key string) uint64 {
	t.Helper()
	out, err := v.Uint64(key)
	require.NoError(t, err)
	return out
}

func TestAddressAndMethods(t *testing.T) {
	h := newHarness(t)
	require.Equal(t, common.HexToAddress("0x0000000000000000000000000000000000001012"), h.precompile.Address())
	require.Equal(t, "layerxVerify", h.precompile.GetName())
	for _, name := range []string{
		layerxverify.VerifyEd25519Method, layerxverify.VerifyReceiptMethod,
		layerxverify.VerifyReceiptInclusionMethod, layerxverify.VerifyStateProofMethod,
		layerxverify.VerifyDiscoveryProofMethod,
	} {
		require.True(t, h.method(name).IsConstant(), name)
	}
	require.Len(t, h.precompile.GetABI().Methods, 5)
}

func TestVerifyEd25519(t *testing.T) {
	h := newHarness(t)
	for _, v := range h.cases("ed25519") {
		domain := layerxverify.RawMessageDomain
		if _, ok := v.Fields["domain"]; ok {
			domain = uint8(number(t, v, "domain")) //nolint:gosec
		}
		out, err := h.call(layerxverify.VerifyEd25519Method, array(t, v, "public_key"), domain,
			bytesOf(t, v, "message"), bytesOf(t, v, "signature"))
		require.NoError(t, err, v.Name)
		require.Equal(t, v.Valid, out[0].(bool), v.Name)
	}
	valid := h.cases("ed25519")[0]
	require.True(t, valid.Valid)
	// The same signature under a neighbouring domain tag is a different digest.
	out, err := h.call(layerxverify.VerifyEd25519Method, array(t, valid, "public_key"),
		uint8(number(t, valid, "domain")+1), bytesOf(t, valid, "message"), bytesOf(t, valid, "signature")) //nolint:gosec
	require.NoError(t, err)
	require.False(t, out[0].(bool))
	for name, args := range map[string][]interface{}{
		"unknown domain":  {array(t, valid, "public_key"), uint8(20), bytesOf(t, valid, "message"), bytesOf(t, valid, "signature")},
		"short signature": {array(t, valid, "public_key"), uint8(2), bytesOf(t, valid, "message"), bytesOf(t, valid, "signature")[:63]},
		"long signature":  {array(t, valid, "public_key"), uint8(2), bytesOf(t, valid, "message"), append(bytesOf(t, valid, "signature"), 0)},
		"oversize message": {array(t, valid, "public_key"), layerxverify.RawMessageDomain,
			make([]byte, 1_048_577), bytesOf(t, valid, "signature")},
	} {
		_, err := h.call(layerxverify.VerifyEd25519Method, args...)
		require.ErrorIs(t, err, vm.ErrExecutionReverted, name)
	}
}

type receiptFacts = struct {
	ReceiptDigest      [32]byte `json:"receiptDigest"`
	ActivityId         [32]byte `json:"activityId"` //nolint:revive,stylecheck
	GlobalSequence     uint64   `json:"globalSequence"`
	ResultCode         int32    `json:"resultCode"`
	ModuleId           uint16   `json:"moduleId"` //nolint:revive,stylecheck
	Operation          uint8    `json:"operation"`
	Asset              [32]byte `json:"asset"`
	Amount             *big.Int `json:"amount"`
	From               [32]byte `json:"from"`
	To                 [32]byte `json:"to"`
	PreviousStateRoot  [32]byte `json:"previousStateRoot"`
	ResultingStateRoot [32]byte `json:"resultingStateRoot"`
	Timestamp          uint64   `json:"timestamp"`
}

func TestVerifyReceipt(t *testing.T) {
	h := newHarness(t)
	for _, v := range h.cases("receipts") {
		out, err := h.call(layerxverify.VerifyReceiptMethod, bytesOf(t, v, "receipt"), array(t, v, "public_key"))
		if !v.Valid {
			require.ErrorIs(t, err, vm.ErrExecutionReverted, v.Name)
			continue
		}
		require.NoError(t, err, v.Name)
		facts := *abi.ConvertType(out[0], new(receiptFacts)).(*receiptFacts)
		require.Equal(t, array(t, v, "digest"), facts.ReceiptDigest, v.Name)
		require.Equal(t, array(t, v, "activity_id"), facts.ActivityId, v.Name)
		require.Equal(t, array(t, v, "asset"), facts.Asset, v.Name)
		require.Equal(t, array(t, v, "resulting_state_root"), facts.ResultingStateRoot, v.Name)
		require.Equal(t, number(t, v, "global_sequence"), facts.GlobalSequence, v.Name)
		require.Equal(t, number(t, v, "amount"), facts.Amount.Uint64(), v.Name)
		require.Equal(t, number(t, v, "timestamp"), facts.Timestamp, v.Name)
		require.EqualValues(t, number(t, v, "module_id"), facts.ModuleId, v.Name)
		require.EqualValues(t, number(t, v, "operation"), facts.Operation, v.Name)
	}
}

type batchFacts = struct {
	HeaderDigest       [32]byte `json:"headerDigest"`
	NetworkId          uint32   `json:"networkId"` //nolint:revive,stylecheck
	Epoch              uint64   `json:"epoch"`
	BatchNumber        uint64   `json:"batchNumber"`
	FirstSequence      uint64   `json:"firstSequence"`
	LastSequence       uint64   `json:"lastSequence"`
	PreviousStateRoot  [32]byte `json:"previousStateRoot"`
	ResultingStateRoot [32]byte `json:"resultingStateRoot"`
	ReceiptRoot        [32]byte `json:"receiptRoot"`
	SequencerId        [32]byte `json:"sequencerId"` //nolint:revive,stylecheck
	TimestampMs        uint64   `json:"timestampMs"`
}

func inclusionArgs(t *testing.T, v testvectors.Vector) []interface{} {
	t.Helper()
	return []interface{}{
		bytesOf(t, v, "receipt"), bytesOf(t, v, "proof"), bytesOf(t, v, "header"), bytesOf(t, v, "header_signature"),
		array(t, v, "sequencer_id"), array(t, v, "public_key"), number(t, v, "first_batch"), number(t, v, "last_batch"),
	}
}

func TestVerifyReceiptInclusion(t *testing.T) {
	h := newHarness(t)
	for _, v := range h.cases("inclusion") {
		out, err := h.call(layerxverify.VerifyReceiptInclusionMethod, inclusionArgs(t, v)...)
		if !v.Valid {
			require.ErrorIs(t, err, vm.ErrExecutionReverted, v.Name)
			continue
		}
		require.NoError(t, err, v.Name)
		facts := *abi.ConvertType(out[0], new(receiptFacts)).(*receiptFacts)
		batch := *abi.ConvertType(out[1], new(batchFacts)).(*batchFacts)
		require.Equal(t, array(t, v, "header_digest"), batch.HeaderDigest, v.Name)
		require.Equal(t, array(t, v, "receipt_root"), batch.ReceiptRoot, v.Name)
		require.Equal(t, array(t, v, "resulting_state_root"), batch.ResultingStateRoot, v.Name)
		require.Equal(t, number(t, v, "batch_number"), batch.BatchNumber, v.Name)
		require.Equal(t, array(t, v, "sequencer_id"), batch.SequencerId, v.Name)
		require.Equal(t, batch.ResultingStateRoot, facts.ResultingStateRoot, v.Name)
		require.GreaterOrEqual(t, facts.GlobalSequence, batch.FirstSequence, v.Name)
		require.LessOrEqual(t, facts.GlobalSequence, batch.LastSequence, v.Name)
	}
	valid := h.cases("inclusion")[0]
	args := inclusionArgs(t, valid)
	args[3] = args[3].([]byte)[:63]
	_, err := h.call(layerxverify.VerifyReceiptInclusionMethod, args...)
	require.ErrorIs(t, err, vm.ErrExecutionReverted)
	args = inclusionArgs(t, valid)
	args[6], args[7] = uint64(9), uint64(8)
	_, err = h.call(layerxverify.VerifyReceiptInclusionMethod, args...)
	require.ErrorIs(t, err, vm.ErrExecutionReverted)
}

func TestVerifyStateProof(t *testing.T) {
	h := newHarness(t)
	for _, v := range h.cases("state") {
		out, err := h.call(layerxverify.VerifyStateProofMethod, bytesOf(t, v, "witness"), array(t, v, "state_root"))
		if !v.Valid {
			require.ErrorIs(t, err, vm.ErrExecutionReverted, v.Name)
			continue
		}
		require.NoError(t, err, v.Name)
		require.EqualValues(t, number(t, v, "module_id"), out[0].(uint16), v.Name)
		require.Equal(t, bytesOf(t, v, "key"), out[1].([]byte), v.Name)
		require.Equal(t, bytesOf(t, v, "value"), out[2].([]byte), v.Name)
	}
}

type discoveryFacts = struct {
	Digest            [32]byte `json:"digest"`
	Version           uint32   `json:"version"`
	CodeHash          [32]byte `json:"codeHash"`
	AbiVersion        uint16   `json:"abiVersion"`
	ObservedSequence  uint64   `json:"observedSequence"`
	ObservedAt        uint64   `json:"observedAt"`
	ValidThrough      uint64   `json:"validThrough"`
	StateRoot         [32]byte `json:"stateRoot"`
	HeadReceiptDigest [32]byte `json:"headReceiptDigest"`
}

func TestVerifyDiscoveryProof(t *testing.T) {
	h := newHarness(t)
	for _, v := range h.cases("discovery") {
		out, err := h.call(layerxverify.VerifyDiscoveryProofMethod, bytesOf(t, v, "payload"),
			bytesOf(t, v, "proof_material"), array(t, v, "program_id"), number(t, v, "staleness_ms"),
			array(t, v, "public_key"))
		if !v.Valid {
			require.ErrorIs(t, err, vm.ErrExecutionReverted, v.Name)
			continue
		}
		require.NoError(t, err, v.Name)
		head := *abi.ConvertType(out[0], new(discoveryFacts)).(*discoveryFacts)
		require.Equal(t, array(t, v, "digest"), head.Digest, v.Name)
		require.Equal(t, array(t, v, "state_root"), head.StateRoot, v.Name)
		require.Equal(t, array(t, v, "code_hash"), head.CodeHash, v.Name)
		require.Equal(t, array(t, v, "head_receipt_digest"), head.HeadReceiptDigest, v.Name)
		require.EqualValues(t, number(t, v, "version"), head.Version, v.Name)
		require.EqualValues(t, number(t, v, "abi_version"), head.AbiVersion, v.Name)
		require.Equal(t, number(t, v, "observed_sequence"), head.ObservedSequence, v.Name)
		require.Equal(t, number(t, v, "observed_at"), head.ObservedAt, v.Name)
		require.Equal(t, number(t, v, "valid_through"), head.ValidThrough, v.Name)
	}
}

func TestRejectsValueDelegateCallAndUnknownSelector(t *testing.T) {
	h := newHarness(t)
	v := h.cases("state")[0]
	input := h.input(layerxverify.VerifyStateProofMethod, bytesOf(t, v, "witness"), array(t, v, "state_root"))
	_, err := h.precompile.Run(h.evm, common.Address{}, common.Address{}, input, big.NewInt(1), true, false, nil)
	require.ErrorIs(t, err, vm.ErrExecutionReverted)
	_, err = h.precompile.Run(h.evm, common.Address{}, common.Address{}, input, nil, true, true, nil)
	require.ErrorIs(t, err, vm.ErrExecutionReverted)
	_, err = h.precompile.Run(h.evm, common.Address{}, common.Address{}, []byte{0xde, 0xad, 0xbe, 0xef}, nil, true, false, nil)
	require.ErrorIs(t, err, vm.ErrExecutionReverted)
	res, err := h.precompile.Run(h.evm, common.Address{}, common.Address{}, input, nil, true, false, nil)
	require.NoError(t, err)
	require.NotEmpty(t, res)
}

// RequiredGas is what the EVM deducts before Run; it must equal the documented
// formula for every method, from calldata alone.
func TestRequiredGasFollowsDocumentedFormula(t *testing.T) {
	h := newHarness(t)
	require.EqualValues(t, 3000, layerxverify.BaseGas)
	require.EqualValues(t, 16, layerxverify.GasPerByte)
	require.EqualValues(t, 4000, layerxverify.GasPerSignature)
	require.EqualValues(t, 100, layerxverify.GasPerProofNode)

	ed := h.cases("ed25519")[0]
	input := h.input(layerxverify.VerifyEd25519Method, array(t, ed, "public_key"), uint8(number(t, ed, "domain")), //nolint:gosec
		bytesOf(t, ed, "message"), bytesOf(t, ed, "signature"))
	body := uint64(len(input) - 4)
	require.Equal(t, 3000+16*body+4000, h.precompile.RequiredGas(input))

	receipt := h.cases("receipts")[0]
	input = h.input(layerxverify.VerifyReceiptMethod, bytesOf(t, receipt, "receipt"), array(t, receipt, "public_key"))
	require.Equal(t, 3000+16*uint64(len(input)-4)+4000, h.precompile.RequiredGas(input))

	for _, v := range h.cases("inclusion")[:3] {
		input = h.input(layerxverify.VerifyReceiptInclusionMethod, inclusionArgs(t, v)...)
		nodes := uint64(len(bytesOf(t, v, "proof"))) / 32
		require.NotZero(t, nodes)
		require.Equal(t, 3000+16*uint64(len(input)-4)+2*4000+100*nodes, h.precompile.RequiredGas(input), v.Name)
	}

	for _, v := range h.cases("state")[:2] {
		witness := bytesOf(t, v, "witness")
		input = h.input(layerxverify.VerifyStateProofMethod, witness, array(t, v, "state_root"))
		require.Equal(t, 3000+16*uint64(len(input)-4)+100*(uint64(len(witness))/32), h.precompile.RequiredGas(input), v.Name)
	}

	discovery := h.cases("discovery")[0]
	input = h.input(layerxverify.VerifyDiscoveryProofMethod, bytesOf(t, discovery, "payload"),
		bytesOf(t, discovery, "proof_material"), array(t, discovery, "program_id"),
		number(t, discovery, "staleness_ms"), array(t, discovery, "public_key"))
	require.Equal(t, 3000+16*uint64(len(input)-4)+4000, h.precompile.RequiredGas(input))

	// A refused proof costs exactly what an accepted proof of the same shape costs.
	var accepted, refused testvectors.Vector
	for _, v := range h.cases("state") {
		switch v.Name {
		case "valid-account":
			accepted = v
		case "wrong-root":
			refused = v
		}
	}
	require.Equal(t,
		h.precompile.RequiredGas(h.input(layerxverify.VerifyStateProofMethod, bytesOf(t, accepted, "witness"), array(t, accepted, "state_root"))),
		h.precompile.RequiredGas(h.input(layerxverify.VerifyStateProofMethod, bytesOf(t, refused, "witness"), array(t, refused, "state_root"))))

	// Calldata that does not ABI-decode still pays base, bytes and signatures.
	garbage := append(append([]byte(nil), h.method(layerxverify.VerifyReceiptInclusionMethod).ID...), make([]byte, 40)...)
	require.Equal(t, uint64(3000+16*40+2*4000), h.precompile.RequiredGas(garbage))
	require.Equal(t, layerxverify.Gas(40, 2, 0), h.precompile.RequiredGas(garbage))
	require.Equal(t, pcommon.UnknownMethodCallGas, h.precompile.RequiredGas([]byte{1, 2, 3, 4, 5}))
}
