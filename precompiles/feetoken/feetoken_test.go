package feetoken_test

import (
	"math/big"
	"testing"

	"github.com/ethereum/go-ethereum/accounts/abi"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/vm"
	tmtypes "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/types"
	"github.com/sidiora-labs/paxeer-network/modules/evm/state"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	pcommon "github.com/sidiora-labs/paxeer-network/precompiles/common"
	"github.com/sidiora-labs/paxeer-network/precompiles/feetoken"
	"github.com/sidiora-labs/paxeer-network/precompiles/utils"
	storetypes "github.com/sidiora-labs/paxeer-network/sdk/store/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

type harness struct {
	t          *testing.T
	precompile *pcommon.Precompile
	db         *state.DBImpl
	evm        *vm.EVM
	caller     common.Address
}

func newHarness(t *testing.T) *harness {
	t.Helper()
	app := testkeeper.EVMTestApp
	ctx, _ := app.NewContext(false, tmtypes.Header{}).WithBlockHeight(11).CacheContext()
	params := types.DefaultParams()
	params.FeeTokenEnabled = true
	params.AllowedFeeDenoms = []types.AllowedFeeDenom{{Denom: "usid", Rate: sdk.NewDec(types.InitialSidioraBaseUnitsPerPax), RateUpdateHeight: ctx.BlockHeight()}, {Denom: "uasset", Rate: sdk.NewDec(1_000_000), RateUpdateHeight: ctx.BlockHeight()}}
	app.EvmKeeper.SetParams(ctx, params)
	p, err := feetoken.NewPrecompile(app.GetPrecompileKeepers())
	require.NoError(t, err)
	db := state.NewDBImpl(ctx, &app.EvmKeeper, true)
	return &harness{t: t, precompile: p, db: db, evm: &vm.EVM{StateDB: db}, caller: common.HexToAddress("0x1234")}
}

func (h *harness) input(name string, args ...interface{}) []byte {
	h.t.Helper()
	input, err := h.precompile.GetABI().Pack(name, args...)
	require.NoError(h.t, err)
	return input
}

func (h *harness) call(name string, args ...interface{}) []byte {
	h.t.Helper()
	result, err := h.precompile.Run(h.evm, h.caller, h.caller, h.input(name, args...), nil, false, false, nil)
	require.NoError(h.t, err)
	return result
}

func (h *harness) denom(account common.Address) string {
	h.t.Helper()
	result, err := h.precompile.Run(h.evm, h.caller, h.caller, h.input(feetoken.GetFeeDenomMethod, account), nil, true, false, nil)
	require.NoError(h.t, err)
	out, err := h.precompile.GetABI().Unpack(feetoken.GetFeeDenomMethod, result)
	require.NoError(h.t, err)
	return out[0].(string)
}

func (h *harness) store() map[string]string {
	h.t.Helper()
	entries := make(map[string]string)
	iter := h.db.Ctx().KVStore(testkeeper.EVMTestApp.GetKey(types.StoreKey)).Iterator(nil, nil)
	defer iter.Close()
	for ; iter.Valid(); iter.Next() {
		entries[string(iter.Key())] = string(iter.Value())
	}
	return entries
}

func TestFeeDenomRoundTripAndCallerIsolation(t *testing.T) {
	h := newHarness(t)
	other := common.HexToAddress("0x5678")
	require.Equal(t, "uhpx", h.denom(h.caller))
	h.call(feetoken.SetFeeDenomMethod, "usid")
	require.Equal(t, "usid", h.denom(h.caller))
	require.Equal(t, "uhpx", h.denom(other))
	require.Equal(t, "usid", testkeeper.EVMTestApp.EvmKeeper.GetAccountFeeDenom(h.db.Ctx(), h.caller))
	h.call(feetoken.SetFeeDenomMethod, "uasset")
	require.Equal(t, "uasset", h.denom(h.caller))
	h.call(feetoken.ClearFeeDenomMethod)
	require.Equal(t, "uhpx", h.denom(h.caller))
	require.False(t, h.db.Ctx().KVStore(testkeeper.EVMTestApp.GetKey(types.StoreKey)).Has(types.AccountFeeDenomKey(h.caller)))
	h.call(feetoken.ClearFeeDenomMethod)
	require.Equal(t, "uhpx", h.denom(h.caller))
}

func TestFeeDenomRefusalsPreservePreference(t *testing.T) {
	for _, tc := range []struct {
		name     string
		denom    string
		disabled bool
		readOnly bool
		delegate bool
		value    *big.Int
		reason   string
	}{
		{name: "disallowed", denom: "unknown", reason: "denom \"unknown\" is not allowed"},
		{name: "empty", denom: "", reason: "denom \"\" is not allowed"},
		{name: "network coin", denom: "uhpx", reason: "denom \"uhpx\" is not allowed"},
		{name: "disabled", denom: "uasset", disabled: true, reason: "fee-token switch is off"},
		{name: "staticcall", denom: "uasset", readOnly: true, reason: "staticcall"},
		{name: "delegatecall", denom: "uasset", delegate: true, reason: "delegatecall"},
		{name: "payment", denom: "uasset", value: big.NewInt(1), reason: "non-payable"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			h := newHarness(t)
			h.call(feetoken.SetFeeDenomMethod, "usid")
			if tc.disabled {
				testkeeper.EVMTestApp.EvmKeeper.Paramstore.Set(h.db.Ctx(), types.KeyFeeTokenEnabled, false)
			}
			before := h.store()
			result, err := h.precompile.Run(h.evm, h.caller, h.caller, h.input(feetoken.SetFeeDenomMethod, tc.denom), tc.value, tc.readOnly, tc.delegate, nil)
			require.ErrorIs(t, err, vm.ErrExecutionReverted)
			reason, err := abi.UnpackRevert(result)
			require.NoError(t, err)
			require.Contains(t, reason, tc.reason)
			require.Equal(t, before, h.store())
			require.Equal(t, "usid", h.denom(h.caller))
		})
	}
}

func TestFeeDenomClearAfterGovernanceChanges(t *testing.T) {
	for _, disable := range []bool{false, true} {
		h := newHarness(t)
		h.call(feetoken.SetFeeDenomMethod, "usid")
		k := &testkeeper.EVMTestApp.EvmKeeper
		k.Paramstore.Set(h.db.Ctx(), types.KeyAllowedFeeDenoms, []types.AllowedFeeDenom{})
		k.Paramstore.Set(h.db.Ctx(), types.KeyFeeTokenEnabled, !disable)
		require.Equal(t, "usid", h.denom(h.caller))
		h.call(feetoken.ClearFeeDenomMethod)
		require.Equal(t, "uhpx", h.denom(h.caller))
	}
}

func TestFeeDenomViewDoesNotWriteOrChargeAnteGas(t *testing.T) {
	h := newHarness(t)
	h.call(feetoken.SetFeeDenomMethod, "usid")
	for _, account := range []common.Address{h.caller, common.HexToAddress("0x5678")} {
		before := h.store()
		meter := sdk.NewGasMeter(0, 1, 1)
		h.db.WithCtx(h.db.Ctx().WithGasMeter(meter))
		require.NotEmpty(t, h.denom(account))
		require.Zero(t, meter.GasConsumed())
		h.db.WithCtx(h.db.Ctx().WithGasMeter(sdk.NewInfiniteGasMeter(1, 1)))
		require.Equal(t, before, h.store())
	}
	require.True(t, h.precompile.GetABI().Methods[feetoken.GetFeeDenomMethod].IsConstant())
}

func TestFeeDenomGasAndTransactions(t *testing.T) {
	h := newHarness(t)
	executor := h.precompile.GetExecutor().(*feetoken.PrecompileExecutor)
	for _, tc := range []struct {
		name        string
		args        []interface{}
		transaction bool
		bytes       uint64
		gas         uint64
	}{
		{feetoken.SetFeeDenomMethod, []interface{}{"usid"}, true, 96, 9536},
		{feetoken.SetFeeDenomMethod, []interface{}{"factory/pax1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq/asset"}, true, 128, 10048},
		{feetoken.GetFeeDenomMethod, []interface{}{h.caller}, false, 32, 3512},
		{feetoken.ClearFeeDenomMethod, nil, true, 0, 8000},
	} {
		input := h.input(tc.name, tc.args...)
		require.Equal(t, tc.bytes, uint64(len(input)-4))
		require.Equal(t, tc.gas, h.precompile.RequiredGas(input))
		require.Equal(t, tc.transaction, executor.IsTransaction(tc.name))
	}
	require.False(t, executor.IsTransaction("unknown"))
	for _, name := range []string{feetoken.SetFeeDenomMethod, feetoken.GetFeeDenomMethod, feetoken.ClearFeeDenomMethod} {
		var input []byte
		switch name {
		case feetoken.SetFeeDenomMethod:
			input = h.input(name, "usid")
		case feetoken.GetFeeDenomMethod:
			input = h.input(name, h.caller)
		default:
			input = h.input(name)
		}
		gas := h.precompile.RequiredGas(input)
		before := h.store()
		_, left, err := vm.RunPrecompiledContract(h.precompile, h.evm, h.caller, h.caller, input, gas-1, nil, nil, name == feetoken.GetFeeDenomMethod, false)
		require.ErrorIs(t, err, vm.ErrOutOfGas)
		require.Zero(t, left)
		require.Equal(t, before, h.store())
		_, left, err = vm.RunPrecompiledContract(h.precompile, h.evm, h.caller, h.caller, input, gas+7, nil, nil, name == feetoken.GetFeeDenomMethod, false)
		require.NoError(t, err)
		require.Equal(t, uint64(7), left)
	}
}

func TestFeeDenomStaticClearAndRollback(t *testing.T) {
	h := newHarness(t)
	h.call(feetoken.SetFeeDenomMethod, "usid")
	result, err := h.precompile.Run(h.evm, h.caller, h.caller, h.input(feetoken.ClearFeeDenomMethod), nil, true, false, nil)
	require.ErrorIs(t, err, vm.ErrExecutionReverted)
	reason, err := abi.UnpackRevert(result)
	require.NoError(t, err)
	require.Contains(t, reason, "staticcall")
	require.Equal(t, "usid", h.denom(h.caller))
	snapshot := h.db.Snapshot()
	h.call(feetoken.ClearFeeDenomMethod)
	require.Equal(t, "uhpx", h.denom(h.caller))
	h.db.RevertToSnapshot(snapshot)
	require.Equal(t, "usid", h.denom(h.caller))
	snapshot = h.db.Snapshot()
	h.call(feetoken.SetFeeDenomMethod, "uasset")
	h.db.RevertToSnapshot(snapshot)
	require.Equal(t, "usid", h.denom(h.caller))
}

func TestFeeDenomStoreOutOfGasPropagates(t *testing.T) {
	for _, name := range []string{feetoken.SetFeeDenomMethod, feetoken.ClearFeeDenomMethod} {
		t.Run(name, func(t *testing.T) {
			h := newHarness(t)
			h.call(feetoken.SetFeeDenomMethod, "usid")
			before := h.store()
			var args []interface{}
			var limit uint64
			descriptor := storetypes.GasDeleteDesc
			if name == feetoken.SetFeeDenomMethod {
				args = []interface{}{"uasset"}
				meter := sdk.NewInfiniteGasMeter(1, 1)
				ctx := h.db.Ctx().WithGasMeter(meter)
				k := &testkeeper.EVMTestApp.EvmKeeper
				require.True(t, k.GetFeeTokenEnabled(ctx))
				allowed, _ := k.IsAllowedFeeDenom(ctx, "uasset")
				require.True(t, allowed)
				limit = meter.GasConsumed()
				descriptor = storetypes.GasWriteCostFlatDesc
			}
			input := h.input(name, args...)
			meter := sdk.NewGasMeter(limit, 1, 1)
			h.db.WithCtx(h.db.Ctx().WithGasMeter(meter))
			require.PanicsWithValue(t, sdk.ErrorOutOfGas{Descriptor: descriptor}, func() {
				_, _ = h.precompile.Run(h.evm, h.caller, h.caller, input, nil, false, false, nil)
			})
			require.True(t, meter.IsPastLimit())
			h.db.WithCtx(h.db.Ctx().WithGasMeter(sdk.NewInfiniteGasMeter(1, 1)))
			require.Equal(t, before, h.store())
			require.Equal(t, "usid", h.denom(h.caller))
		})
	}
}

func TestFeeDenomMissingKeeper(t *testing.T) {
	_, err := feetoken.NewPrecompile(&utils.EmptyKeepers{})
	require.ErrorContains(t, err, "fee-token EVM keeper")
}
