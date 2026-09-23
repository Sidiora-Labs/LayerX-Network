package keeper_test

import (
	"encoding/binary"
	"math/big"
	"testing"
	"time"

	"github.com/ethereum/go-ethereum/common"
	tmtypes "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/types"
	"github.com/sidiora-labs/paxeer-network/layerxproof/codec"
	"github.com/sidiora-labs/paxeer-network/layerxproof/testvectors"
	custodykeeper "github.com/sidiora-labs/paxeer-network/modules/layerxcustody/keeper"
	custodytypes "github.com/sidiora-labs/paxeer-network/modules/layerxcustody/types"
	"github.com/sidiora-labs/paxeer-network/modules/layerxexchange/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/layerxexchange/types"
	"github.com/sidiora-labs/paxeer-network/sdk/store/cachemulti"
	"github.com/sidiora-labs/paxeer-network/sdk/store/dbadapter"
	storetypes "github.com/sidiora-labs/paxeer-network/sdk/store/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	authtypes "github.com/sidiora-labs/paxeer-network/sdk/x/auth/types"
	govtypes "github.com/sidiora-labs/paxeer-network/sdk/x/gov/types"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
	tmdb "github.com/tendermint/tm-db"
)

const (
	tokenDenom     = "ufoo"
	marginBatch    = uint64(40)
	perpsBatch     = uint64(41)
	promotedBatch  = uint64(42)
	mutatedBatch   = uint64(43)
	unknownBatch   = uint64(99)
	eventNamespace = "paxprotocol.paxchain.layerxexchange."
)

var (
	tokenPointer = common.HexToAddress("0x00000000000000000000000000000000000f00f0")
	genesisTime  = time.Unix(1_800_000_000, 0).UTC()
	account      = [32]byte{0xbe, 0xef, 0x01}
	marketID     = [32]byte{0x6d, 0x01}
	orderID      = [32]byte{0x0d, 0x02}
	positionID   = [32]byte{0x0e, 0x03}
)

// withStore adds the exchange store to ctx's multistore. The application
// mounts it only once the v6.5 store upgrade wires the module; until then the
// keeper runs over the app's real stores plus its own.
func withStore(t *testing.T, ctx sdk.Context, key storetypes.StoreKey) sdk.Context {
	t.Helper()
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

func mounted(parent storetypes.MultiStore, key storetypes.StoreKey) (store storetypes.KVStore, ok bool) {
	defer func() {
		if recover() != nil {
			store, ok = nil, false
		}
	}()
	return parent.GetKVStore(key), true
}

// encodeWitness writes a version-2 native state witness for a non-account key
// whose module subtree holds one leaf.
func encodeWitness(moduleID uint16, key, value []byte, moduleSiblings [][32]byte) []byte {
	out := binary.BigEndian.AppendUint16(nil, 2)
	out = binary.BigEndian.AppendUint16(out, moduleID)
	out = binary.BigEndian.AppendUint32(out, uint32(len(key))) //nolint:gosec
	out = append(out, key...)
	out = binary.BigEndian.AppendUint32(out, uint32(len(value))) //nolint:gosec
	out = append(out, value...)
	out = binary.BigEndian.AppendUint32(out, 0)
	out = binary.BigEndian.AppendUint32(out, 1)
	out = append(out, 0)
	out = binary.BigEndian.AppendUint32(out, 9)
	out = append(out, byte(len(moduleSiblings)))
	for _, sibling := range moduleSiblings {
		out = append(out, sibling[:]...)
	}
	return out
}

// perpsWitness proves key/value in the perps module and returns its root.
func perpsWitness(t *testing.T, key, value []byte) ([]byte, [32]byte) {
	t.Helper()
	witness := encodeWitness(types.PerpsModuleID, key, value, [][32]byte{{0x71}, {0x72}, {0x73}, {0x74}})
	decoded, err := codec.DecodeStateWitness(witness)
	require.NoError(t, err)
	root, err := decoded.Root()
	require.NoError(t, err)
	return witness, root
}

type fixture struct {
	t         *testing.T
	ctx       sdk.Context
	keeper    *keeper.Keeper
	custody   *custodykeeper.Keeper
	vectors   testvectors.Fixture
	caller    common.Address
	callerAcc sdk.AccAddress
	native    [32]byte
	token     [32]byte
}

func vectorBytes(t *testing.T, v testvectors.Vector, key string) []byte {
	t.Helper()
	out, err := v.Bytes(key)
	require.NoError(t, err)
	return out
}

func vectorArray(t *testing.T, v testvectors.Vector, key string) [32]byte {
	t.Helper()
	out, err := v.Array32(key)
	require.NoError(t, err)
	return out
}

func vector(t *testing.T, fixture testvectors.Fixture, section, name string) testvectors.Vector {
	t.Helper()
	for _, candidate := range fixture[section] {
		if candidate.Name == name {
			return candidate
		}
	}
	t.Fatalf("no %s vector %s", section, name)
	return testvectors.Vector{}
}

func newFixture(t *testing.T) *fixture {
	t.Helper()
	app := testkeeper.EVMTestApp
	base, _ := app.NewContext(false, tmtypes.Header{}).WithBlockHeight(11).WithBlockTime(genesisTime).CacheContext()
	key := sdk.NewKVStoreKey(types.StoreKey)
	ctx := withStore(t, base, key)
	vectors, err := testvectors.Load()
	require.NoError(t, err)
	f := &fixture{t: t, ctx: ctx, custody: app.LayerXCustodyKeeper, vectors: vectors,
		native: vectorArray(t, vectors["withdrawal"][0], "asset"), token: vectorArray(t, vectors["exit"][0], "asset")}
	f.keeper = keeper.NewKeeper(app.AppCodec(), key, app.LayerXCustodyKeeper, &app.EvmKeeper)
	// Finalized roots come from custody's authority-registered checkpoints;
	// the app wires the anchor module's reader in its place.
	f.custody.SetAnchorReader(nil)
	f.callerAcc, f.caller = testkeeper.MockAddressPair()
	app.EvmKeeper.SetAddressMapping(ctx, f.callerAcc, f.caller)
	for _, denom := range []string{sdk.MustGetBaseDenom(), tokenDenom} {
		coins := sdk.NewCoins(sdk.NewCoin(denom, sdk.NewInt(50_000_000)))
		require.NoError(t, app.BankKeeper.MintCoins(ctx, "evm", coins))
		require.NoError(t, app.BankKeeper.SendCoinsFromModuleToAccount(ctx, "evm", f.callerAcc, coins))
	}
	f.custody.InitGenesis(ctx, *custodytypes.DefaultGenesis())
	require.NoError(t, f.custody.SetAsset(ctx, custodytypes.AssetMapping{AssetId: custodytypes.Hash32(f.native),
		Denom: sdk.MustGetBaseDenom(), Enabled: true}))
	require.NoError(t, f.custody.SetAsset(ctx, custodytypes.AssetMapping{AssetId: custodytypes.Hash32(f.token),
		Denom: tokenDenom, Pointer: tokenPointer.Hex(), Enabled: true}))
	f.keeper.InitGenesis(ctx, *types.DefaultGenesis())
	require.NoError(t, f.keeper.SetMarket(ctx, types.Market{MarketId: custodytypes.Hash32(marketID),
		MarginAssetId: custodytypes.Hash32(f.native), Enabled: true}))
	return f
}

func (f *fixture) balance(address sdk.AccAddress, denom string) sdk.Int {
	return testkeeper.EVMTestApp.BankKeeper.GetBalance(f.ctx, address, denom).Amount
}

func (f *fixture) typedEvents(ctx sdk.Context, name string) int {
	count := 0
	for _, event := range ctx.EventManager().Events() {
		if event.Type == name {
			count++
		}
	}
	return count
}

func (f *fixture) fresh() sdk.Context { return f.ctx.WithEventManager(sdk.NewEventManager()) }

func TestDepositMarginMovesFundsOnlyIntoCustody(t *testing.T) {
	f := newFixture(t)
	ctx := f.fresh()
	before := f.balance(f.callerAcc, sdk.MustGetBaseDenom())
	intent, err := f.keeper.DepositMargin(ctx, f.caller, f.callerAcc, f.native, account, sdk.NewInt(2_500))
	require.NoError(t, err)

	require.Equal(t, sdk.NewInt(2_500), f.balance(f.custody.ModuleAddress(), sdk.MustGetBaseDenom()))
	require.Equal(t, before.SubRaw(2_500), f.balance(f.callerAcc, sdk.MustGetBaseDenom()))
	chainID := testkeeper.EVMTestApp.EvmKeeper.ChainID(ctx)
	require.Equal(t, custodytypes.Hash32(types.IntentID(chainID, f.caller, types.IntentKind_INTENT_KIND_DEPOSIT, 1)), intent.IntentId)
	depositID := custodytypes.DepositID(chainID, f.caller, f.native, account, big.NewInt(2_500), 1)
	require.Equal(t, custodytypes.Hash32(depositID), intent.DepositId)
	deposit, found := f.custody.GetDeposit(ctx, depositID)
	require.True(t, found)
	require.Equal(t, custodytypes.Hash32(account), deposit.Beneficiary)
	require.Equal(t, 1, f.typedEvents(ctx, eventNamespace+"EventMarginDeposited"))
	require.Equal(t, 1, f.typedEvents(ctx, "paxprotocol.paxchain.layerxcustody.EventCustodyDeposit"))

	stored, found := f.keeper.GetIntent(ctx, types.IntentID(chainID, f.caller, types.IntentKind_INTENT_KIND_DEPOSIT, 1))
	require.True(t, found)
	require.Equal(t, intent, stored)
	require.Equal(t, types.IntentStatus_INTENT_STATUS_PENDING, stored.Status)
	require.Equal(t, "2500", stored.Amount)
	require.Equal(t, int64(11), stored.Height)
	require.Equal(t, uint64(1), f.keeper.GetOwnerNonce(ctx, f.caller))
	message, broken := custodykeeper.SolvencyInvariant(f.custody)(ctx)
	require.False(t, broken, message)

	// Refusals leave no intent, no nonce and no custody movement behind.
	_, err = f.keeper.DepositMargin(ctx, f.caller, f.callerAcc, f.token, account, sdk.NewInt(10))
	require.ErrorIs(t, err, types.ErrNotMarginAsset)
	_, err = f.keeper.DepositMargin(ctx, f.caller, f.callerAcc, f.native, [32]byte{}, sdk.NewInt(10))
	require.Error(t, err)
	_, err = f.keeper.DepositMargin(ctx, f.caller, f.callerAcc, f.native, account, sdk.NewInt(60_000_000))
	require.Error(t, err)
	require.Equal(t, uint64(1), f.keeper.GetOwnerNonce(ctx, f.caller))
	require.Equal(t, uint64(1), f.keeper.GetIntentCount(ctx))
	require.Equal(t, sdk.NewInt(2_500), f.balance(f.custody.ModuleAddress(), sdk.MustGetBaseDenom()))
	require.Equal(t, 1, f.typedEvents(ctx, eventNamespace+"EventMarginDeposited"))
}

func TestIntentsAreRecordedWithoutMovingFunds(t *testing.T) {
	f := newFixture(t)
	ctx := f.fresh()
	chainID := testkeeper.EVMTestApp.EvmKeeper.ChainID(ctx)
	custodyBefore := f.balance(f.custody.ModuleAddress(), sdk.MustGetBaseDenom())
	callerBefore := f.balance(f.callerAcc, sdk.MustGetBaseDenom())

	withdraw, err := f.keeper.WithdrawMargin(ctx, f.caller, account, f.native, sdk.NewInt(700))
	require.NoError(t, err)
	place, err := f.keeper.PlaceOrder(ctx, f.caller, marketID, types.SideSell, sdk.NewInt(31_000), sdk.NewInt(4), types.TimeInForcePostOnly)
	require.NoError(t, err)
	cancel, err := f.keeper.CancelOrder(ctx, f.caller, orderID)
	require.NoError(t, err)
	settle, err := f.keeper.RequestSettlement(ctx, f.caller, positionID)
	require.NoError(t, err)

	for nonce, intent := range []types.Intent{withdraw, place, cancel, settle} {
		require.Equal(t, uint64(nonce+1), intent.Nonce)
		require.Equal(t, custodytypes.Hash32(types.IntentID(chainID, f.caller, intent.Kind, intent.Nonce)), intent.IntentId)
		require.Equal(t, custodytypes.Address(f.caller), intent.Owner)
	}
	require.Equal(t, types.IntentKind_INTENT_KIND_WITHDRAW, withdraw.Kind)
	require.Equal(t, sdk.MustGetBaseDenom(), withdraw.Denom)
	require.Equal(t, custodytypes.Hash32(marketID), place.MarketId)
	require.Equal(t, uint32(types.SideSell), place.Side)
	require.Equal(t, "31000", place.Price)
	require.Equal(t, "4", place.Quantity)
	require.Equal(t, uint32(types.TimeInForcePostOnly), place.TimeInForce)
	require.Equal(t, custodytypes.Hash32(orderID), cancel.OrderId)
	require.Equal(t, custodytypes.Hash32(positionID), settle.PositionId)
	for _, name := range []string{"EventMarginWithdrawalRequested", "EventOrderPlaced", "EventOrderCancelRequested",
		"EventSettlementRequested"} {
		require.Equal(t, 1, f.typedEvents(ctx, eventNamespace+name), name)
	}
	require.Equal(t, custodyBefore, f.balance(f.custody.ModuleAddress(), sdk.MustGetBaseDenom()))
	require.Equal(t, callerBefore, f.balance(f.callerAcc, sdk.MustGetBaseDenom()))

	tooLarge := sdk.NewIntFromBigInt(new(big.Int).Lsh(big.NewInt(1), 128))
	refusals := map[string]error{}
	_, refusals["unlisted market"] = f.keeper.PlaceOrder(ctx, f.caller, [32]byte{9}, types.SideBuy, sdk.NewInt(1), sdk.NewInt(1), 0)
	_, refusals["side"] = f.keeper.PlaceOrder(ctx, f.caller, marketID, 3, sdk.NewInt(1), sdk.NewInt(1), 0)
	_, refusals["time in force"] = f.keeper.PlaceOrder(ctx, f.caller, marketID, types.SideBuy, sdk.NewInt(1), sdk.NewInt(1), 4)
	_, refusals["zero price"] = f.keeper.PlaceOrder(ctx, f.caller, marketID, types.SideBuy, sdk.ZeroInt(), sdk.NewInt(1), 0)
	_, refusals["u128 quantity"] = f.keeper.PlaceOrder(ctx, f.caller, marketID, types.SideBuy, sdk.NewInt(1), tooLarge, 0)
	_, refusals["zero order"] = f.keeper.CancelOrder(ctx, f.caller, [32]byte{})
	_, refusals["zero position"] = f.keeper.RequestSettlement(ctx, f.caller, [32]byte{})
	_, refusals["non-margin asset"] = f.keeper.WithdrawMargin(ctx, f.caller, account, f.token, sdk.NewInt(1))
	_, refusals["zero amount"] = f.keeper.WithdrawMargin(ctx, f.caller, account, f.native, sdk.ZeroInt())
	_, refusals["zero account"] = f.keeper.WithdrawMargin(ctx, f.caller, [32]byte{}, f.native, sdk.NewInt(1))
	_, refusals["zero owner"] = f.keeper.CancelOrder(ctx, common.Address{}, orderID)
	for name, err := range refusals {
		require.Error(t, err, name)
	}
	require.NoError(t, f.keeper.SetMarket(ctx, types.Market{MarketId: custodytypes.Hash32(marketID),
		MarginAssetId: custodytypes.Hash32(f.native), Enabled: false}))
	_, err = f.keeper.PlaceOrder(ctx, f.caller, marketID, types.SideBuy, sdk.NewInt(1), sdk.NewInt(1), 0)
	require.ErrorIs(t, err, types.ErrUnknownMarket)
	require.Equal(t, uint64(4), f.keeper.GetOwnerNonce(ctx, f.caller))
	require.Equal(t, uint64(4), f.keeper.GetIntentCount(ctx))
}

func TestViewsProveFinalizedState(t *testing.T) {
	f := newFixture(t)
	exit := f.vectors["exit"][0]
	require.NoError(t, f.custody.RegisterCheckpoint(f.ctx, marginBatch, vectorArray(t, exit, "state_root"), [32]byte{1}))
	marketValue := append(append([]byte(nil), marketID[:]...), 0x01, 0x02)
	marketWitness, perpsRoot := perpsWitness(t, types.MarketStateKey(marketID), marketValue)
	require.NoError(t, f.custody.RegisterCheckpoint(f.ctx, perpsBatch, perpsRoot, [32]byte{1}))
	promoted := vector(t, f.vectors, "state", "valid-module-promoted")
	require.NoError(t, f.custody.RegisterCheckpoint(f.ctx, promotedBatch, vectorArray(t, promoted, "state_root"), [32]byte{1}))
	mutated := vector(t, f.vectors, "state", "mutated-value")
	require.NoError(t, f.custody.RegisterCheckpoint(f.ctx, mutatedBatch, vectorArray(t, mutated, "state_root"), [32]byte{1}))

	margin, err := f.keeper.ProveMargin(f.ctx, vectorArray(t, exit, "account"), f.token, marginBatch, vectorBytes(t, exit, "witness"))
	require.NoError(t, err)
	require.Equal(t, vectorArray(t, exit, "state_root"), margin.StateRoot)
	balance := margin.Account.Balance.Bytes()
	require.Equal(t, exit.Fields["balance"], new(big.Int).SetBytes(balance[:]).String())
	_, err = f.keeper.ProveMargin(f.ctx, vectorArray(t, exit, "account"), f.native, marginBatch, vectorBytes(t, exit, "witness"))
	require.ErrorIs(t, err, types.ErrInvalidProof, "another asset")
	_, err = f.keeper.ProveMargin(f.ctx, vectorArray(t, exit, "account"), f.token, unknownBatch, vectorBytes(t, exit, "witness"))
	require.ErrorIs(t, err, types.ErrNotFinalized)

	market, err := f.keeper.ProveMarket(f.ctx, marketID, perpsBatch, marketWitness)
	require.NoError(t, err)
	require.Equal(t, keeper.ProvenState{BatchNumber: perpsBatch, StateRoot: perpsRoot, ModuleID: types.PerpsModuleID,
		Key: types.MarketStateKey(marketID), Value: marketValue}, market)
	orderWitness, orderRoot := perpsWitness(t, types.OrderStateKey(marketID, orderID), []byte{0x0d})
	positionWitness, positionRoot := perpsWitness(t, types.PositionStateKey(marketID, positionID), []byte{0x0e})
	require.NoError(t, f.custody.RegisterCheckpoint(f.ctx, perpsBatch+10, orderRoot, [32]byte{1}))
	require.NoError(t, f.custody.RegisterCheckpoint(f.ctx, perpsBatch+11, positionRoot, [32]byte{1}))
	order, err := f.keeper.ProveOrder(f.ctx, marketID, orderID, perpsBatch+10, orderWitness)
	require.NoError(t, err)
	require.Equal(t, []byte{0x0d}, order.Value)
	position, err := f.keeper.ProvePosition(f.ctx, marketID, positionID, perpsBatch+11, positionWitness)
	require.NoError(t, err)
	require.Equal(t, []byte{0x0e}, position.Value)

	_, err = f.keeper.ProveMarket(f.ctx, [32]byte{0x55}, perpsBatch, marketWitness)
	require.ErrorIs(t, err, types.ErrStateMismatch, "another market's key")
	_, err = f.keeper.ProveOrder(f.ctx, marketID, orderID, perpsBatch, marketWitness)
	require.ErrorIs(t, err, types.ErrStateMismatch, "a market entry is not an order")
	_, err = f.keeper.ProveMarket(f.ctx, marketID, marginBatch, marketWitness)
	require.ErrorIs(t, err, types.ErrInvalidProof, "another batch's root")
	_, err = f.keeper.ProveMarket(f.ctx, marketID, unknownBatch, marketWitness)
	require.ErrorIs(t, err, types.ErrNotFinalized)
	_, err = f.keeper.ProveMarket(f.ctx, marketID, promotedBatch, vectorBytes(t, promoted, "witness"))
	require.ErrorIs(t, err, types.ErrStateMismatch, "a module 3 entry")
	_, err = f.keeper.ProveMarket(f.ctx, marketID, mutatedBatch, vectorBytes(t, mutated, "witness"))
	require.ErrorIs(t, err, types.ErrInvalidProof, "a mutated value")
	_, err = f.keeper.ProveMarket(f.ctx, marketID, perpsBatch, make([]byte, types.MaxWitnessBytes+1))
	require.ErrorIs(t, err, types.ErrEvidenceTooLong)
}

func TestGenesisAndAuthority(t *testing.T) {
	f := newFixture(t)
	ctx := f.fresh()
	_, err := f.keeper.PlaceOrder(ctx, f.caller, marketID, types.SideBuy, sdk.NewInt(5), sdk.NewInt(6), 0)
	require.NoError(t, err)
	_, err = f.keeper.CancelOrder(ctx, f.caller, orderID)
	require.NoError(t, err)
	exported := f.keeper.ExportGenesis(ctx)
	require.NoError(t, exported.Validate())
	require.Len(t, exported.Intents, 2)
	require.Equal(t, []types.OwnerNonce{{Owner: custodytypes.Address(f.caller), Nonce: 2}}, exported.OwnerNonces)

	app := testkeeper.EVMTestApp
	key := sdk.NewKVStoreKey(types.StoreKey)
	other := keeper.NewKeeper(app.AppCodec(), key, app.LayerXCustodyKeeper, &app.EvmKeeper)
	otherCtx := withStore(t, ctx, key)
	other.InitGenesis(otherCtx, *exported)
	require.Equal(t, exported, other.ExportGenesis(otherCtx))
	next, err := other.RequestSettlement(otherCtx, f.caller, positionID)
	require.NoError(t, err)
	require.Equal(t, uint64(3), next.Nonce)

	duplicate := *exported
	duplicate.Intents = append(duplicate.Intents, exported.Intents[0])
	duplicate.IntentCount = 3
	require.Error(t, duplicate.Validate())

	server := keeper.NewMsgServerImpl(f.keeper)
	listing := types.Market{MarketId: custodytypes.Hash32([32]byte{0x77}), MarginAssetId: custodytypes.Hash32(f.token), Enabled: true}
	_, err = server.SetMarket(sdk.WrapSDKContext(ctx), &types.MsgSetMarket{Authority: f.callerAcc.String(), Market: listing})
	require.Error(t, err, "not the authority")
	governance := authtypes.NewModuleAddress(govtypes.ModuleName).String()
	_, err = server.SetMarket(sdk.WrapSDKContext(ctx), &types.MsgSetMarket{Authority: governance, Market: listing})
	require.NoError(t, err)
	require.True(t, f.keeper.GetParams(ctx).MarginAsset(custodytypes.Hash32(f.token)))
	_, err = server.UpdateParams(sdk.WrapSDKContext(ctx), &types.MsgUpdateParams{Authority: governance,
		Params: types.Params{Markets: []types.Market{listing, listing}}})
	require.Error(t, err, "a market listed twice")
}
