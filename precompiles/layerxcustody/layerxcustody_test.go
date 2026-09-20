package layerxcustody_test

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
	"github.com/sidiora-labs/paxeer-network/layerxproof/testvectors"
	"github.com/sidiora-labs/paxeer-network/modules/evm/state"
	custodykeeper "github.com/sidiora-labs/paxeer-network/modules/layerxcustody/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/layerxcustody/types"
	pcommon "github.com/sidiora-labs/paxeer-network/precompiles/common"
	"github.com/sidiora-labs/paxeer-network/precompiles/layerxcustody"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

const tokenDenom = "ufoo"

var (
	tokenPointer = common.HexToAddress("0x00000000000000000000000000000000000f00f0")
	genesisTime  = time.Unix(1_800_000_000, 0).UTC()
	beneficiary  = [32]byte{0xbe, 0xef, 0x01}
	weiPerBase   = big.NewInt(1_000_000_000_000)
	custody      = common.HexToAddress(layerxcustody.LayerXCustodyAddress)
)

// harness drives the precompile through Run, the EVM entry point, against the
// real application keepers on a branch of the test app's state.
type harness struct {
	t          *testing.T
	ctx        sdk.Context
	stateDB    *state.DBImpl
	evm        *vm.EVM
	precompile *pcommon.Precompile
	keeper     *custodykeeper.Keeper
	withdrawal testvectors.Vector
	exit       testvectors.Vector
	caller     common.Address
	callerAcc  sdk.AccAddress
}

func raw(t *testing.T, v testvectors.Vector, key string) []byte {
	t.Helper()
	out, err := v.Bytes(key)
	require.NoError(t, err)
	return out
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

func newHarness(t *testing.T, withdrawalDelay uint64) *harness {
	t.Helper()
	app := testkeeper.EVMTestApp
	ctx, _ := app.NewContext(false, tmtypes.Header{}).WithBlockHeight(11).WithBlockTime(genesisTime).CacheContext()
	fixture, err := testvectors.Load()
	require.NoError(t, err)
	h := &harness{t: t, keeper: app.LayerXCustodyKeeper, withdrawal: fixture["withdrawal"][0], exit: fixture["exit"][0]}
	// This harness exercises custody against its own authority-registered
	// checkpoints; the app wires the anchor module's reader, which
	// TestWithdrawalReadsAnchorFinalizedCheckpoint covers.
	h.keeper.SetAnchorReader(nil)
	h.callerAcc, h.caller = testkeeper.MockAddressPair()
	app.EvmKeeper.SetAddressMapping(ctx, h.callerAcc, h.caller)
	for _, denom := range []string{sdk.MustGetBaseDenom(), tokenDenom} {
		coins := sdk.NewCoins(sdk.NewCoin(denom, sdk.NewInt(50_000_000)))
		require.NoError(t, app.BankKeeper.MintCoins(ctx, "evm", coins))
		require.NoError(t, app.BankKeeper.SendCoinsFromModuleToAccount(ctx, "evm", h.callerAcc, coins))
	}
	h.keeper.InitGenesis(ctx, *types.DefaultGenesis())
	batch := number(t, h.withdrawal, "batch_number")
	params := types.DefaultParams()
	params.NetworkId = uint32(number(t, h.withdrawal, "network_id")) //nolint:gosec
	params.WithdrawalDelaySeconds = withdrawalDelay
	params.SequencerAuthorizations = []types.SequencerAuthorization{{
		SequencerId: h.withdrawal.Fields["sequencer_id"], PublicKey: h.withdrawal.Fields["public_key"],
		FirstBatchNumber: batch, LastBatchNumber: batch}}
	require.NoError(t, h.keeper.SetParams(ctx, params))
	require.NoError(t, h.keeper.SetAsset(ctx, types.AssetMapping{AssetId: h.withdrawal.Fields["asset"],
		Denom: sdk.MustGetBaseDenom(), Enabled: true}))
	require.NoError(t, h.keeper.SetAsset(ctx, types.AssetMapping{AssetId: h.exit.Fields["asset"], Denom: tokenDenom,
		Pointer: tokenPointer.Hex(), Enabled: true}))
	require.NoError(t, h.keeper.RegisterCheckpoint(ctx, batch, array(t, h.withdrawal, "header_state_root"),
		array(t, h.withdrawal, "header_receipt_root")))
	h.precompile, err = layerxcustody.NewPrecompile(app.GetPrecompileKeepers())
	require.NoError(t, err)
	h.at(ctx)
	return h
}

// at rebuilds the EVM state on ctx, as a new transaction would.
func (h *harness) at(ctx sdk.Context) {
	h.stateDB = state.NewDBImpl(ctx.WithEventManager(sdk.NewEventManager()), &testkeeper.EVMTestApp.EvmKeeper, true)
	h.evm = &vm.EVM{StateDB: h.stateDB, TxContext: vm.TxContext{Origin: h.caller}}
	// The state database works on its own branch; everything the test reads
	// or administers goes through that branch.
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
	if value != nil && value.Sign() > 0 {
		// The interpreter moves msg.value to the callee before it runs a
		// precompile; Run is entered after that transfer.
		base := sdk.NewIntFromBigInt(new(big.Int).Quo(value, weiPerBase))
		wei := sdk.NewIntFromBigInt(new(big.Int).Rem(value, weiPerBase))
		require.NoError(h.t, testkeeper.EVMTestApp.BankKeeper.SendCoinsAndWei(h.ctx, h.callerAcc,
			testkeeper.EVMTestApp.EvmKeeper.GetPaxAddressOrDefault(h.ctx, custody), base, wei))
	}
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

func (h *harness) balance(account sdk.AccAddress, denom string) sdk.Int {
	return testkeeper.EVMTestApp.BankKeeper.GetBalance(h.ctx, account, denom).Amount
}

func (h *harness) pax(address common.Address) sdk.AccAddress {
	return testkeeper.EVMTestApp.EvmKeeper.GetPaxAddressOrDefault(h.ctx, address)
}

func (h *harness) logs(signature string) []*ethtypes.Log {
	var out []*ethtypes.Log
	for _, log := range h.stateDB.GetAllLogs() {
		if log.Topics[0] == crypto.Keccak256Hash([]byte(signature)) {
			require.Equal(h.t, custody, log.Address)
			out = append(out, log)
		}
	}
	return out
}

func (h *harness) typedEvents(name string) int {
	count := 0
	for _, event := range h.stateDB.Ctx().EventManager().Events() {
		if event.Type == "paxprotocol.paxchain.layerxcustody."+name {
			count++
		}
	}
	return count
}

func (h *harness) withdrawalArgs() []interface{} {
	return []interface{}{raw(h.t, h.withdrawal, "receipt"), raw(h.t, h.withdrawal, "proof"),
		raw(h.t, h.withdrawal, "header"), raw(h.t, h.withdrawal, "header_signature")}
}

func (h *harness) exitArgs(batch uint64) []interface{} {
	return []interface{}{raw(h.t, h.exit, "witness"), batch, array(h.t, h.exit, "account"), array(h.t, h.exit, "asset"),
		common.BytesToAddress(raw(h.t, h.exit, "recipient")), raw(h.t, h.exit, "recipient_signature")}
}

func (h *harness) solvent() {
	h.t.Helper()
	message, broken := custodykeeper.SolvencyInvariant(h.keeper)(h.ctx)
	require.False(h.t, broken, message)
}

func TestNativeDepositCustodiesValueAndEmitsTheVaultLog(t *testing.T) {
	h := newHarness(t, 0)
	before := h.balance(h.callerAcc, sdk.MustGetBaseDenom())
	value := new(big.Int).Mul(big.NewInt(2_500), weiPerBase)
	out, err := h.run(layerxcustody.DepositMethod, value, false, false, beneficiary)
	require.NoError(t, err)
	depositID := out[0].([32]byte)

	require.Equal(t, sdk.NewInt(2_500), h.balance(h.keeper.ModuleAddress(), sdk.MustGetBaseDenom()))
	require.Equal(t, before.SubRaw(2_500), h.balance(h.callerAcc, sdk.MustGetBaseDenom()))
	require.True(t, h.balance(h.pax(custody), sdk.MustGetBaseDenom()).IsZero())

	assetID := array(t, h.withdrawal, "asset")
	require.Equal(t, types.DepositID(testkeeper.EVMTestApp.EvmKeeper.ChainID(h.ctx), h.caller, assetID, beneficiary,
		big.NewInt(2_500), 1), depositID)
	logs := h.logs("CustodyDeposit(bytes32,bytes32,address,bytes32,uint256,uint64)")
	require.Len(t, logs, 1)
	require.Equal(t, []common.Hash{logs[0].Topics[0], depositID, assetID, common.BytesToHash(h.caller.Bytes())}, logs[0].Topics)
	require.Len(t, logs[0].Data, 96)
	require.Equal(t, beneficiary[:], logs[0].Data[:32])
	require.Equal(t, big.NewInt(2_500), new(big.Int).SetBytes(logs[0].Data[32:64]))
	require.Equal(t, big.NewInt(1), new(big.Int).SetBytes(logs[0].Data[64:96]))
	require.Equal(t, 1, h.typedEvents("EventCustodyDeposit"))

	require.Equal(t, uint64(1), h.view(layerxcustody.DepositCountMethod)[0])
	require.Equal(t, uint64(1), h.view(layerxcustody.DepositNonceMethod, h.caller, assetID)[0])
	require.Equal(t, assetID, h.view(layerxcustody.NativeAssetIDMethod)[0])
	for _, record := range []interface{}{h.view(layerxcustody.GetDepositMethod, depositID)[0],
		h.view(layerxcustody.GetDepositByIndexMethod, uint64(1))[0]} {
		got := *abi.ConvertType(record, new(layerxcustody.DepositRecord)).(*layerxcustody.DepositRecord)
		require.Equal(t, layerxcustody.DepositRecord{DepositId: depositID, Index: 1, Depositor: h.caller,
			Beneficiary: beneficiary, AssetId: assetID, Denom: sdk.MustGetBaseDenom(), Amount: big.NewInt(2_500),
			Nonce: 1, Height: 11}, got)
	}
	asset := *abi.ConvertType(h.view(layerxcustody.GetAssetMethod, assetID)[0], new(layerxcustody.Asset)).(*layerxcustody.Asset)
	require.Equal(t, big.NewInt(2_500), asset.Custodied)
	require.True(t, asset.Enabled)
	h.solvent()

	_, err = h.run(layerxcustody.DepositMethod, new(big.Int).Add(value, big.NewInt(1)), false, false, beneficiary)
	require.Error(t, err, "a wei remainder is not a LayerX amount")
	_, err = h.run(layerxcustody.DepositMethod, nil, false, false, beneficiary)
	require.Error(t, err)
	_, err = h.run(layerxcustody.DepositMethod, value, false, false, [32]byte{})
	require.Error(t, err)
	_, err = h.run(layerxcustody.DepositMethod, value, false, true, beneficiary)
	require.Error(t, err, "delegatecall")
}

func TestTokenDepositThroughThePointerMap(t *testing.T) {
	h := newHarness(t, 0)
	assetID := array(t, h.exit, "asset")
	require.Equal(t, assetID, h.view(layerxcustody.AssetByPointerMethod, tokenPointer)[0])
	out, err := h.call(layerxcustody.DepositTokenMethod, tokenPointer, big.NewInt(4_000), beneficiary)
	require.NoError(t, err)
	require.Equal(t, sdk.NewInt(4_000), h.balance(h.keeper.ModuleAddress(), tokenDenom))
	logs := h.logs("CustodyDeposit(bytes32,bytes32,address,bytes32,uint256,uint64)")
	require.Len(t, logs, 1)
	require.Equal(t, common.Hash(out[0].([32]byte)), logs[0].Topics[1])
	require.Equal(t, common.Hash(assetID), logs[0].Topics[2])
	_, err = h.call(layerxcustody.DepositTokenMethod, common.HexToAddress("0x77"), big.NewInt(1), beneficiary)
	require.Error(t, err, "an unmapped ERC20 is refused")
	_, err = h.run(layerxcustody.DepositTokenMethod, nil, true, false, tokenPointer, big.NewInt(1), beneficiary)
	require.Error(t, err, "staticcall")
	_, err = h.call(layerxcustody.DepositTokenMethod, tokenPointer, big.NewInt(60_000_000), beneficiary)
	require.Error(t, err, "more than the caller holds")
	h.solvent()
}

func TestWithdrawalPaysOnceThroughThePrecompile(t *testing.T) {
	h := newHarness(t, 0)
	_, err := h.run(layerxcustody.DepositMethod, new(big.Int).Mul(big.NewInt(1_000), weiPerBase), false, false, beneficiary)
	require.NoError(t, err)
	recipient := common.BytesToAddress(raw(t, h.withdrawal, "recipient"))

	out, err := h.call(layerxcustody.FinaliseWithdrawalMethod, h.withdrawalArgs()...)
	require.NoError(t, err)
	claimID := out[0].([32]byte)
	nullifier := array(t, h.withdrawal, "nullifier")
	require.Equal(t, types.WithdrawalClaimID(testkeeper.EVMTestApp.EvmKeeper.ChainID(h.ctx), nullifier, recipient), claimID)
	require.Equal(t, sdk.NewInt(1), h.balance(h.pax(recipient), sdk.MustGetBaseDenom()))
	require.Equal(t, sdk.NewInt(999), h.balance(h.keeper.ModuleAddress(), sdk.MustGetBaseDenom()))

	queued := h.logs("ClaimQueued(bytes32,bytes32,bytes32,bytes32,address,uint256,uint64)")
	require.Len(t, queued, 1)
	require.Equal(t, []common.Hash{queued[0].Topics[0], claimID, nullifier, array(t, h.withdrawal, "anchor")}, queued[0].Topics)
	finalised := h.logs("ClaimFinalised(bytes32,bytes32)")
	require.Len(t, finalised, 1)
	require.Equal(t, []common.Hash{finalised[0].Topics[0], claimID, nullifier}, finalised[0].Topics)
	released := h.logs("CustodyRelease(bytes32,bytes32,address,uint256,address)")
	require.Len(t, released, 1)
	require.Equal(t, common.BytesToHash(recipient.Bytes()), released[0].Topics[3])
	require.Equal(t, big.NewInt(1), new(big.Int).SetBytes(released[0].Data[:32]))
	require.Equal(t, custody, common.BytesToAddress(released[0].Data[32:64]))
	require.Equal(t, 1, h.typedEvents("EventClaimFinalised"))
	require.Equal(t, 1, h.typedEvents("EventCustodyRelease"))

	claim := *abi.ConvertType(h.view(layerxcustody.GetClaimMethod, claimID)[0], new(layerxcustody.Claim)).(*layerxcustody.Claim)
	require.Equal(t, uint8(2), claim.Status)
	require.Equal(t, uint8(1), claim.Kind)
	require.Equal(t, recipient, claim.Recipient)
	require.Equal(t, uint8(2), h.view(layerxcustody.NullifierStatusMethod, nullifier)[0])

	_, err = h.call(layerxcustody.FinaliseWithdrawalMethod, h.withdrawalArgs()...)
	require.Error(t, err, "replay")
	_, err = h.call(layerxcustody.RequestWithdrawalMethod, h.withdrawalArgs()...)
	require.Error(t, err, "replay")
	require.Equal(t, sdk.NewInt(1), h.balance(h.pax(recipient), sdk.MustGetBaseDenom()))
	require.Len(t, h.logs("CustodyRelease(bytes32,bytes32,address,uint256,address)"), 1)
	h.solvent()
}

func TestWithdrawalDelayAndRefusalsThroughThePrecompile(t *testing.T) {
	h := newHarness(t, 900)
	_, err := h.run(layerxcustody.DepositMethod, new(big.Int).Mul(big.NewInt(1_000), weiPerBase), false, false, beneficiary)
	require.NoError(t, err)
	recipient := h.pax(common.BytesToAddress(raw(t, h.withdrawal, "recipient")))

	for index, name := range []string{"receipt", "proof", "header", "header signature"} {
		args := h.withdrawalArgs()
		mutated := append([]byte(nil), args[index].([]byte)...)
		mutated[len(mutated)-1] ^= 1
		args[index] = mutated
		_, err = h.call(layerxcustody.RequestWithdrawalMethod, args...)
		require.Error(t, err, name)
		_, err = h.call(layerxcustody.FinaliseWithdrawalMethod, args...)
		require.Error(t, err, name)
	}
	_, err = h.run(layerxcustody.RequestWithdrawalMethod, nil, true, false, h.withdrawalArgs()...)
	require.Error(t, err, "staticcall")
	_, err = h.run(layerxcustody.RequestWithdrawalMethod, big.NewInt(1), false, false, h.withdrawalArgs()...)
	require.Error(t, err, "non-payable")

	out, err := h.call(layerxcustody.RequestWithdrawalMethod, h.withdrawalArgs()...)
	require.NoError(t, err)
	require.Equal(t, uint64(genesisTime.Unix()+900), out[1]) //nolint:gosec
	require.Len(t, h.logs("ClaimQueued(bytes32,bytes32,bytes32,bytes32,address,uint256,uint64)"), 1)
	require.Equal(t, uint8(1), h.view(layerxcustody.NullifierStatusMethod, array(t, h.withdrawal, "nullifier"))[0])
	_, err = h.call(layerxcustody.FinaliseWithdrawalMethod, h.withdrawalArgs()...)
	require.Error(t, err, "delay")
	require.True(t, h.balance(recipient, sdk.MustGetBaseDenom()).IsZero())

	h.at(h.ctx.WithBlockTime(genesisTime.Add(900 * time.Second)))
	_, err = h.call(layerxcustody.FinaliseWithdrawalMethod, h.withdrawalArgs()...)
	require.NoError(t, err)
	require.Equal(t, sdk.NewInt(1), h.balance(recipient, sdk.MustGetBaseDenom()))
	require.Empty(t, h.logs("ClaimQueued(bytes32,bytes32,bytes32,bytes32,address,uint256,uint64)"))
	h.solvent()
}

func TestForcedExitThroughThePrecompile(t *testing.T) {
	h := newHarness(t, 0)
	_, err := h.call(layerxcustody.DepositTokenMethod, tokenPointer, big.NewInt(6_000_000), beneficiary)
	require.NoError(t, err)
	batch := number(t, h.withdrawal, "batch_number") + 2
	stateRoot := array(t, h.exit, "state_root")
	require.NoError(t, h.keeper.RegisterCheckpoint(h.ctx, batch, stateRoot, [32]byte{7}))
	recipient := common.BytesToAddress(raw(t, h.exit, "recipient"))

	require.Equal(t, false, h.view(layerxcustody.ExitEligibleMethod)[0])
	_, err = h.call(layerxcustody.ExecuteForcedExitMethod, h.exitArgs(batch)...)
	require.Error(t, err, "not eligible")
	require.NoError(t, h.keeper.SetEmergency(h.ctx, true))
	h.at(h.ctx)
	require.Equal(t, true, h.view(layerxcustody.ExitEligibleMethod)[0])

	stolen := h.exitArgs(batch)
	stolen[4] = h.caller
	_, err = h.call(layerxcustody.ExecuteForcedExitMethod, stolen...)
	require.Error(t, err, "a recipient the authority did not sign")

	out, err := h.call(layerxcustody.ExecuteForcedExitMethod, h.exitArgs(batch)...)
	require.NoError(t, err)
	claimID := out[0].([32]byte)
	require.Equal(t, sdk.NewInt(5_000_000), h.balance(h.pax(recipient), tokenDenom))
	require.Equal(t, sdk.NewInt(1_000_000), h.balance(h.keeper.ModuleAddress(), tokenDenom))
	executed := h.logs("EmergencyExitExecuted(bytes32,bytes32,bytes32,bytes32,bytes32,address,uint256)")
	require.Len(t, executed, 1)
	require.Equal(t, common.Hash(claimID), executed[0].Topics[1])
	require.Equal(t, common.Hash(stateRoot), executed[0].Topics[3])
	require.Equal(t, big.NewInt(5_000_000), new(big.Int).SetBytes(executed[0].Data[96:128]))
	require.Len(t, h.logs("CustodyRelease(bytes32,bytes32,address,uint256,address)"), 1)
	require.Equal(t, 1, h.typedEvents("EventForcedExitExecuted"))

	_, err = h.call(layerxcustody.ExecuteForcedExitMethod, h.exitArgs(batch)...)
	require.Error(t, err, "replay")
	_, err = h.call(layerxcustody.RequestForcedExitMethod, h.exitArgs(batch)...)
	require.Error(t, err, "replay")
	require.Equal(t, sdk.NewInt(5_000_000), h.balance(h.pax(recipient), tokenDenom))
	h.solvent()
}

func TestGasIsTheDocumentedFormula(t *testing.T) {
	h := newHarness(t, 0)
	input := h.input(layerxcustody.FinaliseWithdrawalMethod, h.withdrawalArgs()...)
	nodes := uint64(len(raw(t, h.withdrawal, "proof"))) / 32
	require.Equal(t, layerxcustody.Gas(uint64(len(input)-4), 2, nodes, 12), h.precompile.RequiredGas(input))
	require.Equal(t, 3000+16*uint64(len(input)-4)+8000+100*nodes+60000, h.precompile.RequiredGas(input))
	input = h.input(layerxcustody.DepositMethod, beneficiary)
	require.Equal(t, uint64(3000+16*32+5000*8), h.precompile.RequiredGas(input))
	input = h.input(layerxcustody.ExecuteForcedExitMethod, h.exitArgs(1)...)
	require.Equal(t, layerxcustody.Gas(uint64(len(input)-4), 1, uint64(len(raw(t, h.exit, "witness")))/32, 12),
		h.precompile.RequiredGas(input))
	input = h.input(layerxcustody.DepositCountMethod)
	require.Equal(t, uint64(3000), h.precompile.RequiredGas(input))
}
