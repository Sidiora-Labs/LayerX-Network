package layerxexchange_test

import (
	"encoding/binary"
	"math/big"
	"testing"
	"time"

	"github.com/ethereum/go-ethereum/accounts/abi"
	"github.com/ethereum/go-ethereum/common"
	ethtypes "github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/core/vm"
	"github.com/ethereum/go-ethereum/crypto"
	tmtypes "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/types"
	"github.com/sidiora-labs/paxeer-network/layerxproof/codec"
	"github.com/sidiora-labs/paxeer-network/layerxproof/testvectors"
	"github.com/sidiora-labs/paxeer-network/modules/evm/state"
	custodykeeper "github.com/sidiora-labs/paxeer-network/modules/layerxcustody/keeper"
	custodytypes "github.com/sidiora-labs/paxeer-network/modules/layerxcustody/types"
	exchangekeeper "github.com/sidiora-labs/paxeer-network/modules/layerxexchange/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/layerxexchange/types"
	pcommon "github.com/sidiora-labs/paxeer-network/precompiles/common"
	"github.com/sidiora-labs/paxeer-network/precompiles/layerxexchange"
	"github.com/sidiora-labs/paxeer-network/precompiles/utils"
	"github.com/sidiora-labs/paxeer-network/sdk/store/cachemulti"
	"github.com/sidiora-labs/paxeer-network/sdk/store/dbadapter"
	storetypes "github.com/sidiora-labs/paxeer-network/sdk/store/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
	tmdb "github.com/tendermint/tm-db"
)

const (
	tokenDenom  = "ufoo"
	marginBatch = uint64(40)
	perpsBatch  = uint64(41)
)

var (
	tokenPointer = common.HexToAddress("0x00000000000000000000000000000000000f00f0")
	genesisTime  = time.Unix(1_800_000_000, 0).UTC()
	account      = [32]byte{0xbe, 0xef, 0x01}
	marketID     = [32]byte{0x6d, 0x01}
	orderID      = [32]byte{0x0d, 0x02}
	positionID   = [32]byte{0x0e, 0x03}
	weiPerBase   = big.NewInt(1_000_000_000_000)
	exchange     = common.HexToAddress(layerxexchange.ExchangeAddress)
)

// keepers are the application's precompile keepers plus the exchange keeper,
// which the application exposes once the v6.5 upgrade wires the module.
type keepers struct {
	utils.Keepers
	exchange *exchangekeeper.Keeper
}

func (k keepers) LayerXExchangeK() *exchangekeeper.Keeper { return k.exchange }

// harness drives the precompile through Run, the EVM entry point, against the
// real application keepers on a branch of the test app's state.
type harness struct {
	t          *testing.T
	ctx        sdk.Context
	stateDB    *state.DBImpl
	evm        *vm.EVM
	precompile *pcommon.Precompile
	keeper     *exchangekeeper.Keeper
	custody    *custodykeeper.Keeper
	exit       testvectors.Vector
	native     [32]byte
	token      [32]byte
	caller     common.Address
	callerAcc  sdk.AccAddress
}

func array(t *testing.T, v testvectors.Vector, key string) [32]byte {
	t.Helper()
	out, err := v.Array32(key)
	require.NoError(t, err)
	return out
}

func raw(t *testing.T, v testvectors.Vector, key string) []byte {
	t.Helper()
	out, err := v.Bytes(key)
	require.NoError(t, err)
	return out
}

func mounted(parent storetypes.MultiStore, key storetypes.StoreKey) (store storetypes.KVStore, ok bool) {
	defer func() {
		if recover() != nil {
			store, ok = nil, false
		}
	}()
	return parent.GetKVStore(key), true
}

// withStore adds the exchange store to ctx's multistore next to the app's.
func withStore(ctx sdk.Context, key storetypes.StoreKey) sdk.Context {
	parent := ctx.MultiStore()
	stores := map[storetypes.StoreKey]storetypes.CacheWrapper{}
	keys := map[string]storetypes.StoreKey{}
	for _, existing := range parent.StoreKeys() {
		if store, ok := mounted(parent, existing); ok {
			stores[existing] = store
			keys[existing.Name()] = existing
		}
	}
	stores[key] = dbadapter.Store{DB: tmdb.NewMemDB()}
	keys[key.Name()] = key
	return ctx.WithMultiStore(cachemulti.NewStore(tmdb.NewMemDB(), stores, keys, nil, nil, nil, 0))
}

// perpsWitness is a version-2 native state witness for one perps-module entry;
// it returns the witness and the state root it proves under.
func perpsWitness(t *testing.T, key, value []byte) ([]byte, [32]byte) {
	t.Helper()
	out := binary.BigEndian.AppendUint16(nil, 2)
	out = binary.BigEndian.AppendUint16(out, types.PerpsModuleID)
	out = binary.BigEndian.AppendUint32(out, uint32(len(key))) //nolint:gosec
	out = append(out, key...)
	out = binary.BigEndian.AppendUint32(out, uint32(len(value))) //nolint:gosec
	out = append(out, value...)
	out = binary.BigEndian.AppendUint32(out, 0)
	out = binary.BigEndian.AppendUint32(out, 1)
	out = append(out, 0)
	out = binary.BigEndian.AppendUint32(out, 9)
	out = append(out, 4)
	for _, sibling := range [][32]byte{{0x71}, {0x72}, {0x73}, {0x74}} {
		out = append(out, sibling[:]...)
	}
	decoded, err := codec.DecodeStateWitness(out)
	require.NoError(t, err)
	root, err := decoded.Root()
	require.NoError(t, err)
	return out, root
}

func newHarness(t *testing.T) *harness {
	t.Helper()
	app := testkeeper.EVMTestApp
	base, _ := app.NewContext(false, tmtypes.Header{}).WithBlockHeight(11).WithBlockTime(genesisTime).CacheContext()
	key := sdk.NewKVStoreKey(types.StoreKey)
	ctx := withStore(base, key)
	fixture, err := testvectors.Load()
	require.NoError(t, err)
	h := &harness{t: t, custody: app.LayerXCustodyKeeper, exit: fixture["exit"][0],
		native: array(t, fixture["withdrawal"][0], "asset"), token: array(t, fixture["exit"][0], "asset")}
	h.keeper = exchangekeeper.NewKeeper(app.AppCodec(), key, app.LayerXCustodyKeeper, &app.EvmKeeper)
	// Finalized roots come from custody's authority-registered checkpoints;
	// the app wires the anchor module's reader in its place.
	h.custody.SetAnchorReader(nil)
	h.callerAcc, h.caller = testkeeper.MockAddressPair()
	app.EvmKeeper.SetAddressMapping(ctx, h.callerAcc, h.caller)
	for _, denom := range []string{sdk.MustGetBaseDenom(), tokenDenom} {
		coins := sdk.NewCoins(sdk.NewCoin(denom, sdk.NewInt(50_000_000)))
		require.NoError(t, app.BankKeeper.MintCoins(ctx, "evm", coins))
		require.NoError(t, app.BankKeeper.SendCoinsFromModuleToAccount(ctx, "evm", h.callerAcc, coins))
	}
	h.custody.InitGenesis(ctx, *custodytypes.DefaultGenesis())
	require.NoError(t, h.custody.SetAsset(ctx, custodytypes.AssetMapping{AssetId: custodytypes.Hash32(h.native),
		Denom: sdk.MustGetBaseDenom(), Enabled: true}))
	require.NoError(t, h.custody.SetAsset(ctx, custodytypes.AssetMapping{AssetId: custodytypes.Hash32(h.token),
		Denom: tokenDenom, Pointer: tokenPointer.Hex(), Enabled: true}))
	h.keeper.InitGenesis(ctx, *types.DefaultGenesis())
	require.NoError(t, h.keeper.SetMarket(ctx, types.Market{MarketId: custodytypes.Hash32([32]byte{0x6d, 0x02}),
		MarginAssetId: custodytypes.Hash32(h.token), Enabled: true}))
	require.NoError(t, h.keeper.SetMarket(ctx, types.Market{MarketId: custodytypes.Hash32(marketID),
		MarginAssetId: custodytypes.Hash32(h.native), Enabled: true}))
	h.precompile, err = layerxexchange.NewPrecompile(keepers{Keepers: app.GetPrecompileKeepers(), exchange: h.keeper})
	require.NoError(t, err)
	h.at(ctx)
	return h
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
	if value != nil && value.Sign() > 0 {
		// The interpreter moves msg.value to the callee before it runs a
		// precompile; Run is entered after that transfer.
		base := sdk.NewIntFromBigInt(new(big.Int).Quo(value, weiPerBase))
		wei := sdk.NewIntFromBigInt(new(big.Int).Rem(value, weiPerBase))
		require.NoError(h.t, testkeeper.EVMTestApp.BankKeeper.SendCoinsAndWei(h.ctx, h.callerAcc, h.pax(exchange), base, wei))
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

func (h *harness) balance(address sdk.AccAddress, denom string) sdk.Int {
	return testkeeper.EVMTestApp.BankKeeper.GetBalance(h.ctx, address, denom).Amount
}

func (h *harness) pax(address common.Address) sdk.AccAddress {
	return testkeeper.EVMTestApp.EvmKeeper.GetPaxAddressOrDefault(h.ctx, address)
}

func (h *harness) logs(signature string) []*ethtypes.Log {
	var out []*ethtypes.Log
	for _, log := range h.stateDB.GetAllLogs() {
		if log.Topics[0] == crypto.Keccak256Hash([]byte(signature)) {
			require.Equal(h.t, exchange, log.Address)
			out = append(out, log)
		}
	}
	return out
}

func (h *harness) typedEvents(name string) int {
	count := 0
	for _, event := range h.stateDB.Ctx().EventManager().Events() {
		if event.Type == "paxprotocol.paxchain.layerxexchange."+name {
			count++
		}
	}
	return count
}

func (h *harness) intentID(kind types.IntentKind, nonce uint64) [32]byte {
	return types.IntentID(testkeeper.EVMTestApp.EvmKeeper.ChainID(h.ctx), h.caller, kind, nonce)
}

func TestDepositMarginCustodiesNativeValue(t *testing.T) {
	h := newHarness(t)
	custody := h.custody.ModuleAddress()
	before := h.balance(h.callerAcc, sdk.MustGetBaseDenom())
	value := new(big.Int).Mul(big.NewInt(3_000), weiPerBase)
	out, err := h.run(layerxexchange.DepositMarginMethod, value, false, false, account)
	require.NoError(t, err)

	intentID := h.intentID(types.IntentKind_INTENT_KIND_DEPOSIT, 1)
	depositID := custodytypes.DepositID(testkeeper.EVMTestApp.EvmKeeper.ChainID(h.ctx), h.caller, h.native, account,
		big.NewInt(3_000), 1)
	require.Equal(t, intentID, out[0].([32]byte))
	require.Equal(t, depositID, out[1].([32]byte))
	require.Equal(t, sdk.NewInt(3_000), h.balance(custody, sdk.MustGetBaseDenom()))
	require.Equal(t, before.SubRaw(3_000), h.balance(h.callerAcc, sdk.MustGetBaseDenom()))
	require.True(t, h.balance(h.pax(exchange), sdk.MustGetBaseDenom()).IsZero(), "the precompile keeps nothing")
	_, found := h.custody.GetDeposit(h.ctx, depositID)
	require.True(t, found)

	logs := h.logs("MarginDeposited(bytes32,bytes32,address,bytes32,uint256,bytes32,uint64)")
	require.Len(t, logs, 1)
	require.Equal(t, []common.Hash{logs[0].Topics[0], intentID, account, common.BytesToHash(h.caller.Bytes())}, logs[0].Topics)
	require.Equal(t, 1, h.typedEvents("EventMarginDeposited"))

	record := *abi.ConvertType(h.view(layerxexchange.GetIntentMethod, intentID)[0],
		new(layerxexchange.Intent)).(*layerxexchange.Intent)
	require.Equal(t, uint8(types.IntentKind_INTENT_KIND_DEPOSIT), record.Kind)
	require.Equal(t, uint8(types.IntentStatus_INTENT_STATUS_PENDING), record.Status)
	require.Equal(t, h.caller, record.Owner)
	require.Equal(t, account, record.Account)
	require.Equal(t, h.native, record.AssetId)
	require.Equal(t, big.NewInt(3_000), record.Amount)
	require.Equal(t, depositID, record.DepositId)
	require.Equal(t, uint64(1), h.view(layerxexchange.IntentNonceMethod, h.caller)[0].(uint64))

	_, err = h.run(layerxexchange.DepositMarginMethod, nil, false, false, account)
	require.Error(t, err, "no value")
	_, err = h.run(layerxexchange.DepositMarginMethod, value, true, false, account)
	require.Error(t, err, "staticcall")
	_, err = h.run(layerxexchange.DepositMarginMethod, value, false, true, account)
	require.Error(t, err, "delegatecall")
}

func TestDepositMarginTokenCustodiesTheDenom(t *testing.T) {
	h := newHarness(t)
	before := h.balance(h.callerAcc, tokenDenom)
	out, err := h.call(layerxexchange.DepositMarginTokenMethod, tokenPointer, big.NewInt(900), account)
	require.NoError(t, err)
	require.Equal(t, h.intentID(types.IntentKind_INTENT_KIND_DEPOSIT, 1), out[0].([32]byte))
	require.Equal(t, sdk.NewInt(900), h.balance(h.custody.ModuleAddress(), tokenDenom))
	require.Equal(t, before.SubRaw(900), h.balance(h.callerAcc, tokenDenom))
	require.Len(t, h.logs("MarginDeposited(bytes32,bytes32,address,bytes32,uint256,bytes32,uint64)"), 1)

	_, err = h.call(layerxexchange.DepositMarginTokenMethod, common.HexToAddress("0x1234"), big.NewInt(900), account)
	require.Error(t, err, "unknown pointer")
	_, err = h.call(layerxexchange.DepositMarginTokenMethod, tokenPointer, big.NewInt(900), [32]byte{})
	require.Error(t, err, "zero account")
	require.Equal(t, sdk.NewInt(900), h.balance(h.custody.ModuleAddress(), tokenDenom))
	_, err = h.run(layerxexchange.DepositMarginTokenMethod, weiPerBase, false, false, tokenPointer, big.NewInt(1), account)
	require.Error(t, err, "value on a non-payable method")
}

func TestOrderIntentsAreRecordedAndEmitted(t *testing.T) {
	h := newHarness(t)
	custodyBefore := h.balance(h.custody.ModuleAddress(), sdk.MustGetBaseDenom())
	out, err := h.call(layerxexchange.PlaceOrderMethod, marketID, types.SideBuy, big.NewInt(30_000), big.NewInt(2),
		types.TimeInForceGoodTillCancelled)
	require.NoError(t, err)
	placed := h.intentID(types.IntentKind_INTENT_KIND_PLACE, 1)
	require.Equal(t, placed, out[0].([32]byte))
	out, err = h.call(layerxexchange.CancelOrderMethod, orderID)
	require.NoError(t, err)
	cancelled := h.intentID(types.IntentKind_INTENT_KIND_CANCEL, 2)
	require.Equal(t, cancelled, out[0].([32]byte))
	out, err = h.call(layerxexchange.RequestSettlementMethod, positionID)
	require.NoError(t, err)
	settled := h.intentID(types.IntentKind_INTENT_KIND_SETTLE, 3)
	require.Equal(t, settled, out[0].([32]byte))

	topic := common.BytesToHash(h.caller.Bytes())
	placedLogs := h.logs("OrderPlaced(bytes32,bytes32,address,uint8,uint256,uint256,uint8,uint64)")
	require.Len(t, placedLogs, 1)
	require.Equal(t, []common.Hash{placedLogs[0].Topics[0], placed, marketID, topic}, placedLogs[0].Topics)
	decoded, err := h.precompile.GetABI().Events[layerxexchange.OrderPlacedEvent].Inputs.NonIndexed().Unpack(placedLogs[0].Data)
	require.NoError(t, err)
	require.Equal(t, []interface{}{types.SideBuy, big.NewInt(30_000), big.NewInt(2), types.TimeInForceGoodTillCancelled,
		uint64(1)}, decoded)
	cancelLogs := h.logs("OrderCancelRequested(bytes32,bytes32,address,uint64)")
	require.Len(t, cancelLogs, 1)
	require.Equal(t, []common.Hash{cancelLogs[0].Topics[0], cancelled, orderID, topic}, cancelLogs[0].Topics)
	settleLogs := h.logs("SettlementRequested(bytes32,bytes32,address,uint64)")
	require.Len(t, settleLogs, 1)
	require.Equal(t, []common.Hash{settleLogs[0].Topics[0], settled, positionID, topic}, settleLogs[0].Topics)
	for _, name := range []string{"EventOrderPlaced", "EventOrderCancelRequested", "EventSettlementRequested"} {
		require.Equal(t, 1, h.typedEvents(name), name)
	}
	require.Equal(t, custodyBefore, h.balance(h.custody.ModuleAddress(), sdk.MustGetBaseDenom()))

	_, err = h.call(layerxexchange.PlaceOrderMethod, [32]byte{0x99}, types.SideBuy, big.NewInt(1), big.NewInt(1), uint8(0))
	require.Error(t, err, "unlisted market")
	_, err = h.call(layerxexchange.PlaceOrderMethod, marketID, uint8(7), big.NewInt(1), big.NewInt(1), uint8(0))
	require.Error(t, err, "side")
	_, err = h.run(layerxexchange.CancelOrderMethod, nil, true, false, orderID)
	require.Error(t, err, "staticcall")
	require.Equal(t, uint64(3), h.view(layerxexchange.IntentNonceMethod, h.caller)[0].(uint64))
}

func TestWithdrawMarginRecordsARequestAndMovesNothing(t *testing.T) {
	h := newHarness(t)
	custodyBefore := h.balance(h.custody.ModuleAddress(), sdk.MustGetBaseDenom())
	callerBefore := h.balance(h.callerAcc, sdk.MustGetBaseDenom())
	out, err := h.call(layerxexchange.WithdrawMarginMethod, account, h.native, big.NewInt(1_234))
	require.NoError(t, err)
	intentID := h.intentID(types.IntentKind_INTENT_KIND_WITHDRAW, 1)
	require.Equal(t, intentID, out[0].([32]byte))
	logs := h.logs("MarginWithdrawalRequested(bytes32,bytes32,address,bytes32,uint256,uint64)")
	require.Len(t, logs, 1)
	require.Equal(t, []common.Hash{logs[0].Topics[0], intentID, account, common.BytesToHash(h.caller.Bytes())}, logs[0].Topics)
	require.Equal(t, 1, h.typedEvents("EventMarginWithdrawalRequested"))
	require.Equal(t, custodyBefore, h.balance(h.custody.ModuleAddress(), sdk.MustGetBaseDenom()))
	require.Equal(t, callerBefore, h.balance(h.callerAcc, sdk.MustGetBaseDenom()))

	_, err = h.call(layerxexchange.WithdrawMarginMethod, account, [32]byte{0x42}, big.NewInt(1))
	require.Error(t, err, "unknown asset")
	_, err = h.call(layerxexchange.WithdrawMarginMethod, account, h.native, new(big.Int).Lsh(big.NewInt(1), 128))
	require.Error(t, err, "beyond u128")
}

func TestStateViewsVerifyFinalizedWitnesses(t *testing.T) {
	h := newHarness(t)
	marketValue := []byte{0x6d, 0x01, 0x02}
	witness, root := perpsWitness(t, types.MarketStateKey(marketID), marketValue)
	require.NoError(t, h.custody.RegisterCheckpoint(h.ctx, perpsBatch, root, [32]byte{1}))
	out := h.view(layerxexchange.GetMarketMethod, marketID, perpsBatch, witness)
	var record layerxexchange.StateRecord
	record = *abi.ConvertType(out[0], new(layerxexchange.StateRecord)).(*layerxexchange.StateRecord)
	require.Equal(t, layerxexchange.StateRecord{BatchNumber: perpsBatch, StateRoot: root,
		Key: types.MarketStateKey(marketID), Value: marketValue}, record)

	orderWitness, orderRoot := perpsWitness(t, types.OrderStateKey(marketID, orderID), []byte{0x0d})
	require.NoError(t, h.custody.RegisterCheckpoint(h.ctx, perpsBatch+1, orderRoot, [32]byte{1}))
	out = h.view(layerxexchange.GetOrderMethod, marketID, orderID, perpsBatch+1, orderWitness)
	record = *abi.ConvertType(out[0], new(layerxexchange.StateRecord)).(*layerxexchange.StateRecord)
	require.Equal(t, []byte{0x0d}, record.Value)
	positionWitness, positionRoot := perpsWitness(t, types.PositionStateKey(marketID, positionID), []byte{0x0e})
	require.NoError(t, h.custody.RegisterCheckpoint(h.ctx, perpsBatch+2, positionRoot, [32]byte{1}))
	out = h.view(layerxexchange.GetPositionMethod, marketID, positionID, perpsBatch+2, positionWitness)
	record = *abi.ConvertType(out[0], new(layerxexchange.StateRecord)).(*layerxexchange.StateRecord)
	require.Equal(t, []byte{0x0e}, record.Value)

	_, err := h.run(layerxexchange.GetMarketMethod, nil, true, false, marketID, perpsBatch+1, witness)
	require.Error(t, err, "another batch's root")
	_, err = h.run(layerxexchange.GetMarketMethod, nil, true, false, marketID, uint64(99), witness)
	require.Error(t, err, "unfinalized batch")
	_, err = h.run(layerxexchange.GetPositionMethod, nil, true, false, marketID, orderID, perpsBatch+1, orderWitness)
	require.Error(t, err, "an order entry is not a position")
	_, err = h.run(layerxexchange.GetMarketMethod, weiPerBase, true, false, marketID, perpsBatch, witness)
	require.Error(t, err, "value on a view")
}

func TestMarginViewVerifiesTheAccountLeaf(t *testing.T) {
	h := newHarness(t)
	require.NoError(t, h.custody.RegisterCheckpoint(h.ctx, marginBatch, array(t, h.exit, "state_root"), [32]byte{1}))
	out := h.view(layerxexchange.GetMarginMethod, array(t, h.exit, "account"), h.token, marginBatch, raw(t, h.exit, "witness"))
	margin := *abi.ConvertType(out[0], new(layerxexchange.Margin)).(*layerxexchange.Margin)
	require.Equal(t, array(t, h.exit, "account"), margin.Account)
	require.Equal(t, h.token, margin.AssetId)
	require.Equal(t, array(t, h.exit, "state_root"), margin.StateRoot)
	require.Equal(t, h.exit.Fields["balance"], margin.Balance.String())
	require.False(t, margin.Frozen)

	_, err := h.run(layerxexchange.GetMarginMethod, nil, true, false, [32]byte{0x01}, h.token, marginBatch,
		raw(t, h.exit, "witness"))
	require.Error(t, err, "another account")
	_, err = h.run(layerxexchange.GetMarginMethod, nil, true, false, array(t, h.exit, "account"), h.token, uint64(98),
		raw(t, h.exit, "witness"))
	require.Error(t, err, "unfinalized batch")
}

func TestRequiredGasFollowsTheFormula(t *testing.T) {
	h := newHarness(t)
	witness := make([]byte, 320)
	input := h.input(layerxexchange.GetMarginMethod, account, h.token, marginBatch, witness)
	require.Equal(t, layerxexchange.Gas(uint64(len(input)-4), 0, 10, 0), h.precompile.RequiredGas(input))
	input = h.input(layerxexchange.PlaceOrderMethod, marketID, types.SideBuy, big.NewInt(1), big.NewInt(1), uint8(0))
	require.Equal(t, layerxexchange.Gas(uint64(len(input)-4), 0, 0, 3), h.precompile.RequiredGas(input))
	input = h.input(layerxexchange.DepositMarginMethod, account)
	require.Equal(t, layerxexchange.Gas(uint64(len(input)-4), 0, 0, 11), h.precompile.RequiredGas(input))
	require.Equal(t, uint64(3000+16*100+4000*2+100*3+5000*4), layerxexchange.Gas(100, 2, 3, 4))
}

func TestMissingKeeperRefuses(t *testing.T) {
	h := newHarness(t)
	precompile, err := layerxexchange.NewPrecompile(keepers{Keepers: testkeeper.EVMTestApp.GetPrecompileKeepers(), exchange: nil})
	require.NoError(t, err)
	_, err = precompile.Run(h.evm, h.caller, h.caller, h.input(layerxexchange.CancelOrderMethod, orderID), nil, false, false, nil)
	require.ErrorIs(t, err, vm.ErrExecutionReverted)
}
