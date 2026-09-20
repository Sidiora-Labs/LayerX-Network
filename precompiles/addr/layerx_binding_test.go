package addr_test

import (
	"crypto/ed25519"
	"crypto/rand"
	"encoding/binary"
	"math/big"
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/vm"
	"github.com/ethereum/go-ethereum/crypto"
	tmtypes "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/types"
	"github.com/sidiora-labs/paxeer-network/layerxproof/testvectors"
	"github.com/sidiora-labs/paxeer-network/modules/evm/state"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	"github.com/sidiora-labs/paxeer-network/precompiles/addr"
	pcommon "github.com/sidiora-labs/paxeer-network/precompiles/common"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

type layerXHarness struct {
	t       *testing.T
	p       *pcommon.DynamicGasPrecompile
	stateDB *state.DBImpl
	evm     *vm.EVM
	chainID *big.Int
}

func newLayerXHarness(t *testing.T, cosmosChainID string) *layerXHarness {
	t.Helper()
	testApp := testkeeper.EVMTestApp
	ctx := testApp.NewContext(false, tmtypes.Header{}).WithBlockHeight(2).WithChainID(cosmosChainID)
	ctx = ctx.WithMultiStore(ctx.MultiStore().CacheMultiStore())
	k := &testApp.EvmKeeper
	p, err := addr.NewPrecompile(testApp.GetPrecompileKeepers())
	require.NoError(t, err)
	stateDB := state.NewDBImpl(ctx, k, true)
	return &layerXHarness{
		t:       t,
		p:       p,
		stateDB: stateDB,
		evm:     &vm.EVM{StateDB: stateDB},
		chainID: k.ChainID(ctx),
	}
}

type layerXCall struct {
	caller   common.Address
	value    *big.Int
	readOnly bool
	delegate bool
	gas      uint64
}

// run packs the call, sends it through RunAndCalculateGas and returns the
// output, the gas used and the precompile's own refusal reason.
func (h *layerXHarness) run(call layerXCall, method string, args ...interface{}) ([]byte, uint64, error) {
	h.t.Helper()
	input, err := h.p.ABI.Pack(method, args...)
	require.NoError(h.t, err)
	supplied := call.gas
	if supplied == 0 {
		supplied = 200000
	}
	h.stateDB.SetPrecompileError(nil)
	snapshot := h.stateDB.Snapshot()
	ret, remaining, err := h.p.RunAndCalculateGas(h.evm, call.caller, call.caller, input, supplied, call.value, nil, call.readOnly, call.delegate)
	if err != nil {
		require.Equal(h.t, vm.ErrExecutionReverted, err)
		require.Nil(h.t, ret)
		h.stateDB.RevertToSnapshot(snapshot)
		return nil, 0, h.stateDB.GetPrecompileError()
	}
	require.LessOrEqual(h.t, remaining, supplied)
	return ret, supplied - remaining, nil
}

func (h *layerXHarness) unpack(method string, ret []byte) []interface{} {
	h.t.Helper()
	out, err := h.p.ABI.Methods[method].Outputs.Unpack(ret)
	require.NoError(h.t, err)
	return out
}

func (h *layerXHarness) nonce(evmAddress common.Address) uint64 {
	h.t.Helper()
	ret, _, err := h.run(layerXCall{caller: evmAddress, readOnly: true}, addr.LayerXBindNonceMethod, evmAddress)
	require.NoError(h.t, err)
	return h.unpack(addr.LayerXBindNonceMethod, ret)[0].(uint64)
}

func newDidKey(t *testing.T) ([32]byte, ed25519.PrivateKey) {
	t.Helper()
	public, private, err := ed25519.GenerateKey(rand.Reader)
	require.NoError(t, err)
	var didPublicKey [32]byte
	copy(didPublicKey[:], public)
	return didPublicKey, private
}

func (h *layerXHarness) sign(private ed25519.PrivateKey, evmAddress common.Address, nonce uint64) []byte {
	h.t.Helper()
	message, err := types.LayerXBindMessage(h.chainID, evmAddress, nonce)
	require.NoError(h.t, err)
	return ed25519.Sign(private, message)
}

func TestLayerXBindThroughPrecompile(t *testing.T) {
	h := newLayerXHarness(t, "layerx-binding-test")
	paxAddress, evmAddress := testkeeper.MockAddressPair()
	didPublicKey, private := newDidKey(t)

	require.Equal(t, uint64(0), h.nonce(evmAddress))
	_, _, err := h.run(layerXCall{caller: evmAddress}, addr.GetLayerXDidMethod, evmAddress)
	require.ErrorContains(t, err, "is not bound to a LayerX DID")
	_, _, err = h.run(layerXCall{caller: evmAddress}, addr.GetEvmAddrByLayerXMethod, didPublicKey)
	require.ErrorContains(t, err, "is not bound to an EVM address")

	// Without an association or a binding the joined lookup returns what exists.
	ret, _, err := h.run(layerXCall{caller: evmAddress, readOnly: true}, addr.GetUnifiedAccountMethod, evmAddress)
	require.NoError(t, err)
	unified := h.unpack(addr.GetUnifiedAccountMethod, ret)
	require.Equal(t, evmAddress, unified[0].(common.Address))
	require.Equal(t, "", unified[1].(string))
	require.Equal(t, [32]byte{}, unified[2].([32]byte))
	require.Equal(t, [32]byte{}, unified[3].([32]byte))

	ret, gasUsed, err := h.run(layerXCall{caller: evmAddress}, addr.BindLayerXMethod, didPublicKey, h.sign(private, evmAddress, 0))
	require.NoError(t, err)
	require.Empty(t, ret)
	require.GreaterOrEqual(t, gasUsed, addr.LayerXBindSignatureGas)

	logs := h.stateDB.GetAllLogs()
	require.Len(t, logs, 1)
	require.Equal(t, common.HexToAddress(addr.AddrAddress), logs[0].Address)
	require.Equal(t, []common.Hash{
		crypto.Keccak256Hash([]byte("LayerXBound(address,bytes32,uint64)")),
		common.BytesToHash(evmAddress.Bytes()),
		common.Hash(didPublicKey),
	}, logs[0].Topics)
	require.Equal(t, common.LeftPadBytes(binary.BigEndian.AppendUint64(nil, 0), 32), logs[0].Data)

	cosmosEvents := 0
	for _, event := range h.stateDB.Ctx().EventManager().Events() {
		if event.Type == types.EventTypeLayerXBound {
			cosmosEvents++
		}
	}
	require.Equal(t, 1, cosmosEvents)

	require.Equal(t, uint64(1), h.nonce(evmAddress))
	ret, _, err = h.run(layerXCall{caller: evmAddress, readOnly: true}, addr.GetLayerXDidMethod, evmAddress)
	require.NoError(t, err)
	did := h.unpack(addr.GetLayerXDidMethod, ret)
	require.Equal(t, didPublicKey, did[0].([32]byte))
	require.Equal(t, types.LayerXDid(didPublicKey), did[1].(string))
	ret, _, err = h.run(layerXCall{caller: evmAddress, readOnly: true}, addr.GetEvmAddrByLayerXMethod, didPublicKey)
	require.NoError(t, err)
	require.Equal(t, evmAddress, h.unpack(addr.GetEvmAddrByLayerXMethod, ret)[0].(common.Address))

	// With the Paxeer association in place the three identities join.
	testkeeper.EVMTestApp.EvmKeeper.SetAddressMapping(h.stateDB.Ctx(), paxAddress, evmAddress)
	ret, _, err = h.run(layerXCall{caller: evmAddress, readOnly: true}, addr.GetUnifiedAccountMethod, evmAddress)
	require.NoError(t, err)
	unified = h.unpack(addr.GetUnifiedAccountMethod, ret)
	mainAccountID, err := types.LayerXMainAccountID(didPublicKey)
	require.NoError(t, err)
	require.Equal(t, evmAddress, unified[0].(common.Address))
	require.Equal(t, paxAddress.String(), unified[1].(string))
	require.Equal(t, didPublicKey, unified[2].([32]byte))
	require.Equal(t, mainAccountID, unified[3].([32]byte))
}

func TestLayerXBindRefusalsThroughPrecompile(t *testing.T) {
	h := newLayerXHarness(t, "layerx-binding-test")
	_, evmAddress := testkeeper.MockAddressPair()
	_, thief := testkeeper.MockAddressPair()
	didPublicKey, private := newDidKey(t)
	signature := h.sign(private, evmAddress, 0)

	_, _, err := h.run(layerXCall{caller: evmAddress, value: big.NewInt(1)}, addr.BindLayerXMethod, didPublicKey, signature)
	require.ErrorContains(t, err, "sending funds to a non-payable function")
	_, _, err = h.run(layerXCall{caller: evmAddress, readOnly: true}, addr.BindLayerXMethod, didPublicKey, signature)
	require.ErrorContains(t, err, "cannot call bindLayerX from staticcall")
	_, _, err = h.run(layerXCall{caller: evmAddress, delegate: true}, addr.BindLayerXMethod, didPublicKey, signature)
	require.ErrorContains(t, err, "cannot delegatecall bindLayerX")
	_, _, err = h.run(layerXCall{caller: evmAddress}, addr.BindLayerXMethod, didPublicKey, signature[:63])
	require.ErrorIs(t, err, types.ErrLayerXSignatureLength)
	_, _, err = h.run(layerXCall{caller: evmAddress, gas: 1}, addr.BindLayerXMethod, didPublicKey, signature)
	require.Error(t, err)

	// msg.sender is what the DID key must have signed.
	_, _, err = h.run(layerXCall{caller: thief}, addr.BindLayerXMethod, didPublicKey, signature)
	require.ErrorIs(t, err, types.ErrLayerXSignature)
	// A signature made for another chain id is refused.
	otherChain, err := types.LayerXBindMessage(new(big.Int).Add(h.chainID, big.NewInt(1)), evmAddress, 0)
	require.NoError(t, err)
	_, _, err = h.run(layerXCall{caller: evmAddress}, addr.BindLayerXMethod, didPublicKey, ed25519.Sign(private, otherChain))
	require.ErrorIs(t, err, types.ErrLayerXSignature)
	// A non-canonical key (y = p) is refused before anything else.
	nonCanonical := [32]byte{0xed}
	for i := 1; i < 31; i++ {
		nonCanonical[i] = 0xff
	}
	nonCanonical[31] = 0x7f
	_, _, err = h.run(layerXCall{caller: evmAddress}, addr.BindLayerXMethod, nonCanonical, signature)
	require.ErrorIs(t, err, types.ErrLayerXNonCanonicalKey)

	require.Equal(t, uint64(0), h.nonce(evmAddress))
	require.Empty(t, h.stateDB.GetAllLogs())

	_, _, err = h.run(layerXCall{caller: evmAddress}, addr.BindLayerXMethod, didPublicKey, signature)
	require.NoError(t, err)

	// One to one in both directions.
	otherKey, otherPrivate := newDidKey(t)
	_, _, err = h.run(layerXCall{caller: evmAddress}, addr.BindLayerXMethod, otherKey, h.sign(otherPrivate, evmAddress, 1))
	require.ErrorIs(t, err, types.ErrLayerXAddressBound)
	_, _, err = h.run(layerXCall{caller: thief}, addr.BindLayerXMethod, didPublicKey, h.sign(private, thief, 0))
	require.ErrorIs(t, err, types.ErrLayerXDidBound)
}

func TestLayerXUnbindAndRebindThroughPrecompile(t *testing.T) {
	h := newLayerXHarness(t, "layerx-binding-test")
	_, evmAddress := testkeeper.MockAddressPair()
	didPublicKey, private := newDidKey(t)
	signature := h.sign(private, evmAddress, 0)

	_, _, err := h.run(layerXCall{caller: evmAddress}, addr.UnbindLayerXMethod)
	require.ErrorIs(t, err, types.ErrLayerXNotBound)
	_, _, err = h.run(layerXCall{caller: evmAddress}, addr.BindLayerXMethod, didPublicKey, signature)
	require.NoError(t, err)

	_, _, err = h.run(layerXCall{caller: evmAddress, readOnly: true}, addr.UnbindLayerXMethod)
	require.ErrorContains(t, err, "cannot call unbindLayerX from staticcall")
	_, _, err = h.run(layerXCall{caller: evmAddress, delegate: true}, addr.UnbindLayerXMethod)
	require.ErrorContains(t, err, "cannot delegatecall unbindLayerX")
	_, _, err = h.run(layerXCall{caller: evmAddress, value: big.NewInt(1)}, addr.UnbindLayerXMethod)
	require.ErrorContains(t, err, "sending funds to a non-payable function")

	ret, _, err := h.run(layerXCall{caller: evmAddress}, addr.UnbindLayerXMethod)
	require.NoError(t, err)
	require.Empty(t, ret)
	logs := h.stateDB.GetAllLogs()
	require.Len(t, logs, 2)
	require.Equal(t, []common.Hash{
		crypto.Keccak256Hash([]byte("LayerXUnbound(address,bytes32,uint64)")),
		common.BytesToHash(evmAddress.Bytes()),
		common.Hash(didPublicKey),
	}, logs[1].Topics)
	require.Equal(t, common.LeftPadBytes(binary.BigEndian.AppendUint64(nil, 1), 32), logs[1].Data)
	require.Equal(t, uint64(2), h.nonce(evmAddress))

	// Replay of the consumed signature is refused; a fresh one at nonce 2 binds.
	_, _, err = h.run(layerXCall{caller: evmAddress}, addr.BindLayerXMethod, didPublicKey, signature)
	require.ErrorIs(t, err, types.ErrLayerXSignature)
	_, _, err = h.run(layerXCall{caller: evmAddress}, addr.BindLayerXMethod, didPublicKey, h.sign(private, evmAddress, 2))
	require.NoError(t, err)
	require.Equal(t, uint64(3), h.nonce(evmAddress))
}

func TestLayerXBindRustSignerVectorThroughPrecompile(t *testing.T) {
	fixture, err := testvectors.LoadPaxeerBind()
	require.NoError(t, err)
	accepted := 0
	for _, bind := range fixture.Binds {
		if bind.Nonce != 0 {
			continue
		}
		cosmosChainID := "layerx-binding-test"
		if bind.ChainID == 125 {
			cosmosChainID = "hyperpax_125-1"
		}
		h := newLayerXHarness(t, cosmosChainID)
		require.Equal(t, bind.ChainID, h.chainID.Uint64(), bind.Name)
		evmAddress := common.Address(bind.EVMAddress)
		_, _, err := h.run(layerXCall{caller: evmAddress}, addr.BindLayerXMethod, fixture.PublicKey, bind.Signature[:])
		if !bind.Valid {
			require.ErrorIs(t, err, types.ErrLayerXSignature, bind.Name)
			continue
		}
		require.NoError(t, err, bind.Name)
		accepted++
		ret, _, err := h.run(layerXCall{caller: evmAddress, readOnly: true}, addr.GetUnifiedAccountMethod, evmAddress)
		require.NoError(t, err)
		unified := h.unpack(addr.GetUnifiedAccountMethod, ret)
		require.Equal(t, fixture.PublicKey, unified[2].([32]byte))
		require.Equal(t, fixture.MainAccountID, unified[3].([32]byte), "main account id differs from the Rust wire crate's")
		ret, _, err = h.run(layerXCall{caller: evmAddress, readOnly: true}, addr.GetLayerXDidMethod, evmAddress)
		require.NoError(t, err)
		require.Equal(t, fixture.Did, h.unpack(addr.GetLayerXDidMethod, ret)[1].(string))
	}
	require.GreaterOrEqual(t, accepted, 1)
}
