package layerxbridge_test

import (
	"math/big"
	"testing"
	"time"

	"github.com/ethereum/go-ethereum/accounts/abi"
	"github.com/ethereum/go-ethereum/common"
	ethtypes "github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/core/vm"
	"github.com/ethereum/go-ethereum/crypto"
	tmtypes "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/types"
	"github.com/sidiora-labs/paxeer-network/modules/evm/state"
	bridgekeeper "github.com/sidiora-labs/paxeer-network/modules/layerxbridge/keeper"
	bridgetestutil "github.com/sidiora-labs/paxeer-network/modules/layerxbridge/testutil"
	"github.com/sidiora-labs/paxeer-network/modules/layerxbridge/types"
	pcommon "github.com/sidiora-labs/paxeer-network/precompiles/common"
	"github.com/sidiora-labs/paxeer-network/precompiles/layerxbridge"
	"github.com/sidiora-labs/paxeer-network/precompiles/utils"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

const chainID = uint64(1)

var (
	vault     = common.HexToAddress("0x1111111111111111111111111111111111111111")
	asset     = common.HexToAddress("0x4444444444444444444444444444444444444444")
	remote    = common.HexToAddress("0x3333333333333333333333333333333333333333")
	bridge    = common.HexToAddress(layerxbridge.BridgeAddress)
	authority = types.DefaultAuthority()
)

// keepers are the application's precompile keepers plus the bridge keeper,
// as the application provides them once the bridge is wired.
type keepers struct {
	utils.Keepers
	bridge *bridgekeeper.Keeper
}

func (k keepers) LayerXBridgeK() *bridgekeeper.Keeper { return k.bridge }

// harness drives the precompile through Run, the EVM entry point, against the
// real application keepers on a branch of the test app's state.
type harness struct {
	t          *testing.T
	ctx        sdk.Context
	stateDB    *state.DBImpl
	evm        *vm.EVM
	precompile *pcommon.Precompile
	keeper     bridgekeeper.Keeper
	attestors  []bridgetestutil.Attestor
	caller     common.Address
	callerAcc  sdk.AccAddress
	denom      string
}

func newHarness(t *testing.T) *harness {
	t.Helper()
	app := testkeeper.EVMTestApp
	ctx, _ := app.NewContext(false, tmtypes.Header{}).WithBlockHeight(11).WithBlockTime(time.Unix(1_800_000_000, 0)).CacheContext()
	k, ctx := bridgetestutil.NewKeeper(app, ctx)
	h := &harness{t: t, keeper: k, attestors: bridgetestutil.Attestors(3), denom: types.Denom(chainID, types.Address20(asset))}
	h.callerAcc, h.caller = testkeeper.MockAddressPair()
	app.EvmKeeper.SetAddressMapping(ctx, h.callerAcc, h.caller)
	k.InitGenesis(ctx, *types.DefaultGenesis())
	var err error
	h.precompile, err = layerxbridge.NewPrecompile(keepers{Keepers: app.GetPrecompileKeepers(), bridge: &h.keeper})
	require.NoError(t, err)
	h.at(ctx)
	return h
}

// activate registers chain 1, three attestors with threshold two and caps of
// 1000 per transaction and 1500 in flight.
func (h *harness) activate() {
	h.t.Helper()
	require.NoError(h.t, h.keeper.RegisterChain(h.ctx, types.MsgRegisterChain{Authority: authority,
		Chain: types.Chain{ChainID: chainID, Vault: types.Address20(vault), FinalityDepth: 64, Enabled: true}}))
	require.NoError(h.t, h.keeper.SetAttestors(h.ctx, types.MsgSetAttestors{Authority: authority,
		Set: bridgetestutil.Set(h.attestors, 1_000_000, 2)}))
	require.NoError(h.t, h.keeper.SetCap(h.ctx, types.MsgSetCap{Authority: authority, ChainID: chainID,
		Asset: types.Address20(asset), MaxInFlight: sdk.NewInt(1500), MaxPerTx: sdk.NewInt(1000)}))
}

// at rebuilds the EVM state on ctx, as a new transaction would.
func (h *harness) at(ctx sdk.Context) {
	h.stateDB = state.NewDBImpl(ctx.WithEventManager(sdk.NewEventManager()), &testkeeper.EVMTestApp.EvmKeeper, true)
	h.evm = &vm.EVM{StateDB: h.stateDB, TxContext: vm.TxContext{Origin: h.caller}}
	h.ctx = h.stateDB.Ctx()
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

func (h *harness) run(name string, value *big.Int, readOnly, delegate bool, args ...interface{}) ([]interface{}, error) {
	h.t.Helper()
	res, err := h.precompile.Run(h.evm, h.caller, h.caller, h.input(name, args...), value, readOnly, delegate, nil)
	if err != nil {
		require.ErrorIs(h.t, err, vm.ErrExecutionReverted)
		reason, unpackErr := abi.UnpackRevert(res)
		require.NoError(h.t, unpackErr)
		require.NotEmpty(h.t, reason)
		return nil, err
	}
	out, err := h.method(name).Outputs.Unpack(res)
	require.NoError(h.t, err)
	return out, nil
}

func (h *harness) call(name string, args ...interface{}) ([]interface{}, error) {
	return h.run(name, nil, false, false, args...)
}

func (h *harness) view(name string, args ...interface{}) []interface{} {
	h.t.Helper()
	out, err := h.run(name, nil, true, false, args...)
	require.NoError(h.t, err)
	return out
}

func (h *harness) balance() sdk.Int {
	return testkeeper.EVMTestApp.BankKeeper.GetBalance(h.ctx, h.callerAcc, h.denom).Amount
}

func (h *harness) logs(signature string) []*ethtypes.Log {
	var out []*ethtypes.Log
	for _, log := range h.stateDB.GetAllLogs() {
		if log.Topics[0] == crypto.Keccak256Hash([]byte(signature)) {
			require.Equal(h.t, bridge, log.Address)
			out = append(out, log)
		}
	}
	return out
}

func (h *harness) events(kind string) int {
	count := 0
	for _, event := range h.stateDB.Ctx().EventManager().Events() {
		if event.Type == kind {
			count++
		}
	}
	return count
}

// bridgeInArgs are the bridgeIn arguments for a deposit to the caller, signed
// by signers.
func (h *harness) bridgeInArgs(logIndex uint64, amount int64, signers ...bridgetestutil.Attestor) []interface{} {
	recipient := common.BytesToHash(h.caller.Bytes())
	txHash := common.Hash{0xde, 0xad, byte(logIndex)}
	in := types.BridgeIn{ChainID: chainID, Vault: types.Address20(vault), TxHash: types.Hash32(txHash),
		LogIndex: logIndex, Recipient: types.Hash32(recipient), Asset: types.Address20(asset), Amount: big.NewInt(amount)}
	return []interface{}{chainID, vault, [32]byte(txHash), logIndex, [32]byte(recipient), asset, big.NewInt(amount),
		bridgetestutil.Sign(types.InboundDigest(in), signers...)}
}

func TestBridgeInMintsAtThresholdThroughThePrecompile(t *testing.T) {
	h := newHarness(t)
	require.Equal(t, false, h.view(layerxbridge.GetChainMethod, chainID)[0], "dormant: nothing registered")
	require.Equal(t, false, h.view(layerxbridge.IsPausedMethod)[0])
	_, err := h.call(layerxbridge.BridgeInMethod, h.bridgeInArgs(7, 600, h.attestors[0], h.attestors[1])...)
	require.Error(t, err, "dormant bridge refuses")

	h.activate()
	out, err := h.call(layerxbridge.BridgeInMethod, h.bridgeInArgs(7, 600, h.attestors[0], h.attestors[1])...)
	require.NoError(t, err)
	require.Equal(t, h.denom, out[0])
	require.Equal(t, sdk.NewInt(600), h.balance())

	logs := h.logs("BridgeIn(uint64,bytes32,address,uint64,address,uint256,string)")
	require.Len(t, logs, 1)
	require.Equal(t, common.BigToHash(big.NewInt(1)), logs[0].Topics[1])
	require.Equal(t, common.Hash{0xde, 0xad, 7}, logs[0].Topics[2])
	require.Equal(t, common.BytesToHash(h.caller.Bytes()), logs[0].Topics[3])
	data, err := h.precompile.GetABI().Events[layerxbridge.BridgeInEvent].Inputs.NonIndexed().Unpack(logs[0].Data)
	require.NoError(t, err)
	require.Equal(t, []interface{}{uint64(7), asset, big.NewInt(600), h.denom}, data)
	require.Equal(t, 1, h.events(types.EventBridgeIn))

	require.Equal(t, true, h.view(layerxbridge.IsNullifiedMethod, chainID, [32]byte{0xde, 0xad, 7}, uint64(7))[0])
	require.Equal(t, []interface{}{h.denom, big.NewInt(1500), big.NewInt(1000), big.NewInt(600)},
		h.view(layerxbridge.GetCapMethod, chainID, asset))
	require.Equal(t, []interface{}{true, vault, uint64(64), true}, h.view(layerxbridge.GetChainMethod, chainID))
	attestors := h.view(layerxbridge.GetAttestorsMethod)
	require.Equal(t, []common.Address{common.Address(h.attestors[0].Signer), common.Address(h.attestors[1].Signer),
		common.Address(h.attestors[2].Signer)}, attestors[0])
	require.Equal(t, []*big.Int{big.NewInt(1_000_000), big.NewInt(1_000_000), big.NewInt(1_000_000)}, attestors[1])
	require.Equal(t, uint32(2), attestors[2])
}

func TestBridgeInRefusalsThroughThePrecompile(t *testing.T) {
	h := newHarness(t)
	h.activate()

	_, err := h.call(layerxbridge.BridgeInMethod, h.bridgeInArgs(1, 100, h.attestors[2])...)
	require.Error(t, err, "below threshold")
	_, err = h.call(layerxbridge.BridgeInMethod, h.bridgeInArgs(1, 1001, h.attestors[0], h.attestors[1])...)
	require.Error(t, err, "over the per-transaction cap")
	unregistered := h.bridgeInArgs(1, 100, h.attestors[0], h.attestors[1])
	unregistered[0] = uint64(9)
	_, err = h.call(layerxbridge.BridgeInMethod, unregistered...)
	require.Error(t, err, "unregistered chain")

	require.NoError(t, h.keeper.Pause(h.ctx, types.MsgPause{Authority: authority}))
	require.Equal(t, true, h.view(layerxbridge.IsPausedMethod)[0])
	_, err = h.call(layerxbridge.BridgeInMethod, h.bridgeInArgs(1, 100, h.attestors[0], h.attestors[1])...)
	require.Error(t, err, "paused")
	require.NoError(t, h.keeper.Unpause(h.ctx, types.MsgUnpause{Authority: authority}))
	require.True(t, h.balance().IsZero())

	_, err = h.call(layerxbridge.BridgeInMethod, h.bridgeInArgs(1, 100, h.attestors[0], h.attestors[1])...)
	require.NoError(t, err)
	_, err = h.call(layerxbridge.BridgeInMethod, h.bridgeInArgs(1, 100, h.attestors[1], h.attestors[2])...)
	require.Error(t, err, "reused nullifier")
	require.Equal(t, sdk.NewInt(100), h.balance())

	_, err = h.run(layerxbridge.BridgeInMethod, nil, true, false, h.bridgeInArgs(2, 100, h.attestors[0], h.attestors[1])...)
	require.Error(t, err, "staticcall cannot write")
	_, err = h.run(layerxbridge.BridgeInMethod, nil, false, true, h.bridgeInArgs(2, 100, h.attestors[0], h.attestors[1])...)
	require.Error(t, err, "delegatecall")
	_, err = h.run(layerxbridge.BridgeInMethod, big.NewInt(1), false, false, h.bridgeInArgs(2, 100, h.attestors[0], h.attestors[1])...)
	require.Error(t, err, "not payable")
	_, err = h.run(layerxbridge.IsPausedMethod, nil, true, true)
	require.Error(t, err, "delegatecall of a view")
	require.Equal(t, sdk.NewInt(100), h.balance())
}

func TestBridgeOutBurnsAndEmitsThroughThePrecompile(t *testing.T) {
	h := newHarness(t)
	h.activate()
	_, err := h.call(layerxbridge.BridgeInMethod, h.bridgeInArgs(1, 900, h.attestors[0], h.attestors[1])...)
	require.NoError(t, err)

	out, err := h.call(layerxbridge.BridgeOutMethod, chainID, asset, big.NewInt(400), remote)
	require.NoError(t, err)
	require.Equal(t, uint64(1), out[0])
	require.Equal(t, sdk.NewInt(500), h.balance())
	require.Equal(t, sdk.NewInt(500), testkeeper.EVMTestApp.BankKeeper.GetSupply(h.ctx, h.denom).Amount)
	require.Equal(t, big.NewInt(500), h.view(layerxbridge.GetCapMethod, chainID, asset)[3])

	logs := h.logs("BridgeOut(uint64,address,uint256,address,uint64)")
	require.Len(t, logs, 1)
	require.Equal(t, []common.Hash{logs[0].Topics[0], common.BigToHash(big.NewInt(1)), common.BytesToHash(asset.Bytes()),
		common.BigToHash(big.NewInt(1))}, logs[0].Topics)
	data, err := h.precompile.GetABI().Events[layerxbridge.BridgeOutEvent].Inputs.NonIndexed().Unpack(logs[0].Data)
	require.NoError(t, err)
	require.Equal(t, []interface{}{big.NewInt(400), remote}, data)
	require.Equal(t, 1, h.events(types.EventBridgeOut))

	out, err = h.call(layerxbridge.BridgeOutMethod, chainID, asset, big.NewInt(500), remote)
	require.NoError(t, err)
	require.Equal(t, uint64(2), out[0])
	require.True(t, h.balance().IsZero())
	_, err = h.call(layerxbridge.BridgeOutMethod, chainID, asset, big.NewInt(1), remote)
	require.Error(t, err, "nothing left to burn")
	_, err = h.run(layerxbridge.BridgeOutMethod, nil, true, false, chainID, asset, big.NewInt(1), remote)
	require.Error(t, err, "staticcall cannot write")
}

func TestGasIsTheDocumentedFormula(t *testing.T) {
	h := newHarness(t)
	input := h.input(layerxbridge.BridgeInMethod, h.bridgeInArgs(1, 100, h.attestors[0], h.attestors[1])...)
	require.Equal(t, layerxbridge.Gas(uint64(len(input)-4), 2, 8), h.precompile.RequiredGas(input))
	require.Equal(t, 3000+16*uint64(len(input)-4)+3000*2+5000*8, h.precompile.RequiredGas(input))
	input = h.input(layerxbridge.BridgeOutMethod, chainID, asset, big.NewInt(1), remote)
	require.Equal(t, uint64(3000+16*128+5000*6), h.precompile.RequiredGas(input))
	input = h.input(layerxbridge.IsNullifiedMethod, chainID, [32]byte{}, uint64(0))
	require.Equal(t, uint64(3000+16*96), h.precompile.RequiredGas(input))
	input = h.input(layerxbridge.IsPausedMethod)
	require.Equal(t, uint64(3000), h.precompile.RequiredGas(input))
}

func TestNewPrecompileNeedsTheBridgeKeeper(t *testing.T) {
	_, err := layerxbridge.NewPrecompile(keepers{Keepers: testkeeper.EVMTestApp.GetPrecompileKeepers(), bridge: nil})
	require.Error(t, err)
	p := layerxbridge.NewPrecompileWithKeeper(bridgekeeper.Keeper{})
	require.Equal(t, bridge, p.Address())
	require.Equal(t, layerxbridge.PrecompileName, p.GetName())
}
