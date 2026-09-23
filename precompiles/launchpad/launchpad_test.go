package launchpad_test

import (
	"encoding/json"
	"math"
	"math/big"
	"testing"
	"time"

	"github.com/ethereum/go-ethereum/accounts/abi"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core"
	ethtypes "github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/core/vm"
	"github.com/ethereum/go-ethereum/crypto"
	"github.com/sidiora-labs/paxeer-network/modules/evm/state"
	evmtypes "github.com/sidiora-labs/paxeer-network/modules/evm/types"
	launchpadkeeper "github.com/sidiora-labs/paxeer-network/modules/launchpad/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/launchpad/launchpadtest"
	"github.com/sidiora-labs/paxeer-network/modules/launchpad/types"
	pcommon "github.com/sidiora-labs/paxeer-network/precompiles/common"
	"github.com/sidiora-labs/paxeer-network/precompiles/launchpad"
	"github.com/sidiora-labs/paxeer-network/precompiles/utils"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

const quote = types.DefaultQuoteDenom

var (
	genesisTime = time.Unix(1_800_000_000, 0).UTC()
	deadline    = big.NewInt(genesisTime.Add(10_000 * time.Hour).Unix())
	precompile  = common.HexToAddress(launchpad.LaunchpadAddress)
)

type keepers struct {
	utils.Keepers
	launchpad *launchpadkeeper.Keeper
}

func (k keepers) LaunchpadK() *launchpadkeeper.Keeper { return k.launchpad }

type harness struct {
	t          *testing.T
	stateDB    *state.DBImpl
	evm        *vm.EVM
	precompile *pcommon.Precompile
	keeper     *launchpadkeeper.Keeper
	caller     common.Address
	callerAcc  sdk.AccAddress
}

func newHarness(t *testing.T) *harness {
	t.Helper()
	app := testkeeper.EVMTestApp
	ctx := app.GetContextForDeliverTx([]byte{}).WithBlockTime(genesisTime)
	ctx = ctx.WithGasMeter(sdk.NewInfiniteGasMeterWithMultiplier(ctx))
	k, ctx := launchpadtest.NewKeeper(app, ctx)
	k.InitGenesis(ctx, *types.DefaultGenesis())
	h := &harness{t: t, keeper: k}
	var err error
	h.precompile, err = launchpad.NewPrecompile(keepers{Keepers: app.GetPrecompileKeepers(), launchpad: k})
	require.NoError(t, err)
	h.stateDB = state.NewDBImpl(ctx, &app.EvmKeeper, false)
	blockCtx, err := app.EvmKeeper.GetVMBlockContext(ctx, core.GasPool(math.MaxUint64))
	require.NoError(t, err)
	cfg := evmtypes.DefaultChainConfig().EthereumConfig(app.EvmKeeper.ChainID(ctx))
	h.evm = vm.NewEVM(*blockCtx, h.stateDB, cfg, vm.Config{}, app.EvmKeeper.CustomPrecompiles(ctx))
	h.callerAcc, h.caller = h.fund(1_000_000_000)
	return h
}

func (h *harness) fund(amount int64) (sdk.AccAddress, common.Address) {
	h.t.Helper()
	acc, address := testkeeper.MockAddressPair()
	testkeeper.EVMTestApp.EvmKeeper.SetAddressMapping(h.stateDB.Ctx(), acc, address)
	coins := sdk.NewCoins(sdk.NewCoin(quote, sdk.NewInt(amount)))
	require.NoError(h.t, testkeeper.EVMTestApp.BankKeeper.MintCoins(h.stateDB.Ctx(), "evm", coins))
	require.NoError(h.t, testkeeper.EVMTestApp.BankKeeper.SendCoinsFromModuleToAccount(h.stateDB.Ctx(), "evm", acc, coins))
	return acc, address
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

func (h *harness) run(from common.Address, name string, value *big.Int, readOnly, delegate bool, args ...interface{}) ([]interface{}, string) {
	h.t.Helper()
	res, err := h.precompile.Run(h.evm, from, from, h.input(name, args...), value, readOnly, delegate, nil)
	if err != nil {
		require.ErrorIs(h.t, err, vm.ErrExecutionReverted)
		reason, unpackErr := abi.UnpackRevert(res)
		require.NoError(h.t, unpackErr)
		require.NotEmpty(h.t, reason)
		return nil, reason
	}
	out, err := h.method(name).Outputs.Unpack(res)
	require.NoError(h.t, err)
	return out, ""
}

func (h *harness) call(from common.Address, name string, args ...interface{}) []interface{} {
	h.t.Helper()
	out, reason := h.run(from, name, nil, false, false, args...)
	require.Empty(h.t, reason, name)
	return out
}

func (h *harness) reverts(from common.Address, name string, args ...interface{}) string {
	h.t.Helper()
	_, reason := h.run(from, name, nil, false, false, args...)
	require.NotEmpty(h.t, reason, name)
	return reason
}

func (h *harness) view(name string, args ...interface{}) []interface{} {
	h.t.Helper()
	out, reason := h.run(h.caller, name, nil, true, false, args...)
	require.Empty(h.t, reason, name)
	return out
}

func (h *harness) balance(acc sdk.AccAddress, denom string) sdk.Int {
	return testkeeper.EVMTestApp.BankKeeper.GetBalance(h.stateDB.Ctx(), acc, denom).Amount
}

func (h *harness) logs(signature string) []*ethtypes.Log {
	var out []*ethtypes.Log
	for _, log := range h.stateDB.GetAllLogs() {
		if log.Topics[0] == crypto.Keccak256Hash([]byte(signature)) {
			require.Equal(h.t, precompile, log.Address)
			out = append(out, log)
		}
	}
	return out
}

func (h *harness) create() (common.Address, string) {
	h.t.Helper()
	out := h.call(h.caller, launchpad.CreateMarketMethod, "Kindle Token", "KNDL", uint8(types.FeeStrategyClaim))
	return out[0].(common.Address), out[1].(string)
}

func (h *harness) market(token common.Address) launchpad.Market {
	h.t.Helper()
	return *abi.ConvertType(h.view(launchpad.GetMarketMethod, token)[0], new(launchpad.Market)).(*launchpad.Market)
}

func sameJSON(t *testing.T, want, got interface{}) {
	t.Helper()
	wantJSON, err := json.Marshal(want)
	require.NoError(t, err)
	gotJSON, err := json.Marshal(got)
	require.NoError(t, err)
	require.JSONEq(t, string(wantJSON), string(gotJSON))
}

func addressTopic(address common.Address) common.Hash { return common.BytesToHash(address.Bytes()) }

func word(data []byte, index int) *big.Int {
	return new(big.Int).SetBytes(data[32*index : 32*index+32])
}

func TestRequiredGasIsPinned(t *testing.T) {
	h := newHarness(t)
	token := common.HexToAddress("0x00000000000000000000000000000000000f00f0")
	for _, tc := range []struct {
		input []byte
		want  uint64
	}{
		{h.input(launchpad.CreateMarketMethod, "Kindle Token", "KNDL", uint8(0)), 3000 + 16*224 + 5000*12 + 9_000_000},
		{h.input(launchpad.BuyMethod, token, big.NewInt(1), big.NewInt(0), token, deadline), 3000 + 16*160 + 5000*8},
		{h.input(launchpad.SellMethod, token, big.NewInt(1), big.NewInt(0), token, deadline), 3000 + 16*160 + 5000*6},
		{h.input(launchpad.SetFeeStrategyMethod, token, uint8(1)), 3000 + 16*64 + 5000},
		{h.input(launchpad.ClaimFeesMethod, token, token), 3000 + 16*64 + 5000*3},
		{h.input(launchpad.ExecuteBurnMethod, token), 3000 + 16*32 + 5000*3},
		{h.input(launchpad.ExecuteAirdropMethod, token), 3000 + 16*32 + 5000*4},
		{h.input(launchpad.ClaimAirdropMethod, token), 3000 + 16*32 + 5000*4},
		{h.input(launchpad.ExecuteLpRewardsMethod, token), 3000 + 16*32 + 5000},
		{h.input(launchpad.PauseMethod, token), 3000 + 16*32 + 5000},
		{h.input(launchpad.UnpauseMethod, token), 3000 + 16*32 + 5000},
		{h.input(launchpad.QuoteBuyMethod, token, big.NewInt(1)), 3000 + 16*64},
		{h.input(launchpad.GetMarketsMethod, big.NewInt(0), big.NewInt(10)), 3000 + 16*64},
		{h.input(launchpad.GetMarketCountMethod), 3000},
		{h.input(launchpad.GetConfigMethod), 3000},
	} {
		require.Equal(t, tc.want, h.precompile.RequiredGas(tc.input))
	}
	require.Equal(t, uint64(9_066_584), h.precompile.RequiredGas(h.input(launchpad.CreateMarketMethod, "Kindle Token", "KNDL", uint8(0))))
	require.Equal(t, uint64(45_560), h.precompile.RequiredGas(h.input(launchpad.BuyMethod, token, big.NewInt(1), big.NewInt(0), token, deadline)))
}

func TestNewPrecompileNeedsTheLaunchpadKeeper(t *testing.T) {
	_, err := launchpad.NewPrecompile(&utils.EmptyKeepers{})
	require.Error(t, err)
	_, err = launchpad.NewPrecompile(keepers{Keepers: testkeeper.EVMTestApp.GetPrecompileKeepers(), launchpad: nil})
	require.Error(t, err)
}

func TestCreateMarketAndTradeThroughThePrecompile(t *testing.T) {
	h := newHarness(t)
	token, denom := h.create()
	require.Equal(t, "factory/"+h.keeper.ModuleAddress().String()+"/lp1", denom)
	pointer, _, exists := testkeeper.EVMTestApp.EvmKeeper.GetERC20NativePointer(h.stateDB.Ctx(), denom)
	require.True(t, exists)
	require.Equal(t, pointer, token)
	require.Equal(t, sdk.NewInt(900_000_000), h.balance(h.callerAcc, quote))
	require.Equal(t, sdk.NewInt(100_000_000), h.balance(h.keeper.TreasuryAddress(), quote))
	created := h.logs("MarketCreated(address,address,string,string,string,uint8)")
	require.Len(t, created, 1)
	require.Equal(t, []common.Hash{created[0].Topics[0], addressTopic(token), addressTopic(h.caller)}, created[0].Topics)

	market := h.market(token)
	sameJSON(t, launchpad.Market{Token: token, Denom: denom, Index: 1, Name: "Kindle Token", Symbol: "KNDL",
		Creator: h.caller, Guardian: h.caller, FeeRightsHolder: h.caller, FeeStrategy: 0, Paused: false,
		TotalSupply: big.NewInt(1_000_000_000_000_000), VirtualQuoteReserve: big.NewInt(10_000_000_000),
		RealQuoteBalance: big.NewInt(0), TokenReserve: big.NewInt(1_000_000_000_000_000),
		CreatedAt: big.NewInt(genesisTime.Unix()), CumulativeVolume: big.NewInt(0), AccumulatedQuoteFees: big.NewInt(0),
		AccumulatedTokenFees: big.NewInt(0), AccumulatedFees: big.NewInt(0), AirdropEpoch: big.NewInt(0),
		AirdropBalance: big.NewInt(0), Price: big.NewInt(10_000_000_000_000)}, market)
	require.Equal(t, big.NewInt(1), h.view(launchpad.GetMarketCountMethod)[0])
	require.Len(t, h.view(launchpad.GetMarketsMethod, big.NewInt(0), big.NewInt(10))[0], 1)
	require.Len(t, h.view(launchpad.GetMarketsMethod, big.NewInt(1), big.NewInt(10))[0], 0)
	require.Len(t, h.view(launchpad.GetMarketsByCreatorMethod, h.caller)[0], 1)
	config := *abi.ConvertType(h.view(launchpad.GetConfigMethod)[0], new(launchpad.Config)).(*launchpad.Config)
	require.Equal(t, quote, config.QuoteDenom)
	require.Equal(t, big.NewInt(1000), config.ProtocolFeeBps)
	require.Equal(t, big.NewInt(100_000_000), config.CreationFee)
	require.Equal(t, big.NewInt(300), h.view(launchpad.GetFeeBpsMethod, token)[0])

	quoted := h.view(launchpad.QuoteBuyMethod, token, big.NewInt(100_000_000))
	require.Equal(t, []interface{}{big.NewInt(9_606_813_905_120), big.NewInt(300), big.NewInt(3_000_000)}, quoted)
	out := h.call(h.caller, launchpad.BuyMethod, token, big.NewInt(100_000_000), big.NewInt(9_606_813_905_120), h.caller, deadline)
	require.Equal(t, big.NewInt(9_606_813_905_120), out[0])
	require.Equal(t, sdk.NewInt(9_606_813_905_120), h.balance(h.callerAcc, denom))
	require.Equal(t, sdk.NewInt(800_000_000), h.balance(h.callerAcc, quote))
	require.Equal(t, sdk.NewInt(100_300_000), h.balance(h.keeper.TreasuryAddress(), quote))
	swaps := h.logs("Swap(address,address,address,bool,uint256,uint256,uint256,uint256)")
	require.Len(t, swaps, 1)
	require.Equal(t, []common.Hash{swaps[0].Topics[0], addressTopic(token), addressTopic(h.caller), addressTopic(h.caller)}, swaps[0].Topics)
	require.Equal(t, big.NewInt(1), word(swaps[0].Data, 0))
	require.Equal(t, big.NewInt(100_000_000), word(swaps[0].Data, 1))
	require.Equal(t, big.NewInt(9_606_813_905_120), word(swaps[0].Data, 2))
	require.Equal(t, big.NewInt(3_000_000), word(swaps[0].Data, 3))
	require.Equal(t, big.NewInt(10_194_940_899_999), word(swaps[0].Data, 4))
	fees := h.logs("FeeRecorded(address,uint256,uint256,uint256)")
	require.Len(t, fees, 1)
	require.Equal(t, big.NewInt(3_000_000), word(fees[0].Data, 0))
	require.Equal(t, big.NewInt(300_000), word(fees[0].Data, 1))
	require.Equal(t, big.NewInt(2_700_000), word(fees[0].Data, 2))

	require.Equal(t, []interface{}{big.NewInt(10_000_000_000), big.NewInt(97_000_000), big.NewInt(990_393_186_094_880)},
		h.view(launchpad.GetReservesMethod, token))
	require.Equal(t, big.NewInt(10_194_940_899_999), h.view(launchpad.GetPriceMethod, token)[0])
	snapshots := h.view(launchpad.GetPriceSnapshotsMethod, token)
	require.Equal(t, big.NewInt(10_194_940_899_999), snapshots[0].([8]*big.Int)[0])
	require.Equal(t, big.NewInt(1), snapshots[1])
	require.Equal(t, big.NewInt(1), snapshots[2])
	require.Equal(t, big.NewInt(2_700_000), h.view(launchpad.GetAccumulatedFeesMethod, token)[0])

	require.Contains(t, h.reverts(h.caller, launchpad.SellMethod, token, big.NewInt(4_803_406_952_560),
		big.NewInt(47_278_913), h.caller, deadline), "slippage")
	require.Equal(t, []interface{}{big.NewInt(47_278_912), big.NewInt(300), big.NewInt(144_102_208_576)},
		h.view(launchpad.QuoteSellMethod, token, big.NewInt(4_803_406_952_560)))
	out = h.call(h.caller, launchpad.SellMethod, token, big.NewInt(4_803_406_952_560), big.NewInt(47_278_912), h.caller, deadline)
	require.Equal(t, big.NewInt(47_278_912), out[0])
	require.Equal(t, sdk.NewInt(847_278_912), h.balance(h.callerAcc, quote))
	market = h.market(token)
	require.Equal(t, big.NewInt(49_721_088), market.RealQuoteBalance)
	require.Equal(t, big.NewInt(144_102_208_576), market.AccumulatedTokenFees)
	require.Len(t, h.logs("FeeRecorded(address,uint256,uint256,uint256)"), 1, "a sell records no quote fee")

	_, other := h.fund(10)
	require.Contains(t, h.reverts(other, launchpad.BuyMethod, token, big.NewInt(1_000_000), big.NewInt(0), other, deadline),
		"insufficient funds")
	require.Contains(t, h.reverts(h.caller, launchpad.BuyMethod, token, big.NewInt(1_000_000), big.NewInt(0), common.Address{}, deadline),
		"zero address")
	require.Contains(t, h.reverts(h.caller, launchpad.BuyMethod, token, big.NewInt(1_000_000), big.NewInt(0), h.caller,
		big.NewInt(genesisTime.Unix()-1)), "deadline")
	require.Contains(t, h.reverts(h.caller, launchpad.BuyMethod, h.caller, big.NewInt(1_000_000), big.NewInt(0), h.caller, deadline),
		"unknown market")
	require.Contains(t, h.reverts(h.caller, launchpad.CreateMarketMethod, "Kindle Token", "KNDL", uint8(4)), "strategy")
	message, broken := launchpadkeeper.SolvencyInvariant(h.keeper)(h.stateDB.Ctx())
	require.False(t, broken, message)
}

func TestFeeRightsAndGuardianThroughThePrecompile(t *testing.T) {
	h := newHarness(t)
	token, denom := h.create()
	_, trader := h.fund(2_000_000_000)
	strangerAcc, stranger := h.fund(0)
	h.call(trader, launchpad.BuyMethod, token, big.NewInt(333_333_334), big.NewInt(0), trader, deadline)
	require.Equal(t, big.NewInt(9_000_000), h.view(launchpad.GetAccumulatedFeesMethod, token)[0])

	require.Contains(t, h.reverts(stranger, launchpad.ClaimFeesMethod, token, stranger), "fee rights")
	require.Contains(t, h.reverts(h.caller, launchpad.ExecuteBurnMethod, token), "strategy")
	out := h.call(h.caller, launchpad.ClaimFeesMethod, token, stranger)
	require.Equal(t, big.NewInt(9_000_000), out[0])
	require.Equal(t, sdk.NewInt(9_000_000), h.balance(strangerAcc, quote))
	claimed := h.logs("FeesClaimed(address,address,uint256)")
	require.Len(t, claimed, 1)
	require.Equal(t, []common.Hash{claimed[0].Topics[0], addressTopic(token), addressTopic(stranger)}, claimed[0].Topics)
	require.Contains(t, h.reverts(h.caller, launchpad.ClaimFeesMethod, token, stranger), "no fees")

	require.Equal(t, uint8(0), h.call(h.caller, launchpad.SetFeeStrategyMethod, token, uint8(types.FeeStrategyAirdrop))[0])
	require.Len(t, h.logs("FeeStrategyChanged(address,uint8,uint8)"), 1)
	require.Contains(t, h.reverts(stranger, launchpad.ClaimAirdropMethod, token), "airdrop not triggered")
	h.call(trader, launchpad.BuyMethod, token, big.NewInt(333_333_334), big.NewInt(0), trader, deadline)
	require.Equal(t, big.NewInt(9_000_000), h.call(h.caller, launchpad.ExecuteAirdropMethod, token)[0])
	airdrops := h.logs("AirdropExecuted(address,uint256,uint256)")
	require.Len(t, airdrops, 1)
	require.Equal(t, big.NewInt(1), word(airdrops[0].Data, 1))
	require.Equal(t, big.NewInt(1), h.market(token).AirdropEpoch)
	traderAcc := testkeeper.EVMTestApp.EvmKeeper.GetPaxAddressOrDefault(h.stateDB.Ctx(), trader)
	holding := h.balance(traderAcc, denom).BigInt()
	want := new(big.Int).Quo(new(big.Int).Mul(big.NewInt(9_000_000), holding), big.NewInt(1_000_000_000_000_000))
	require.Equal(t, want, h.call(trader, launchpad.ClaimAirdropMethod, token)[0])
	require.Contains(t, h.reverts(trader, launchpad.ClaimAirdropMethod, token), "already claimed")
	require.Contains(t, h.reverts(stranger, launchpad.ClaimAirdropMethod, token), "zero amount")
	require.Len(t, h.logs("AirdropClaimed(address,address,uint256,uint256)"), 1)

	h.call(h.caller, launchpad.SetFeeStrategyMethod, token, uint8(types.FeeStrategyLpRewards))
	h.call(trader, launchpad.BuyMethod, token, big.NewInt(333_333_334), big.NewInt(0), trader, deadline)
	real := h.market(token).RealQuoteBalance
	require.Equal(t, big.NewInt(9_000_000), h.call(h.caller, launchpad.ExecuteLpRewardsMethod, token)[0])
	require.Equal(t, new(big.Int).Add(real, big.NewInt(9_000_000)), h.market(token).RealQuoteBalance)
	require.Len(t, h.logs("LpRewardsExecuted(address,uint256)"), 1)

	h.call(h.caller, launchpad.SetFeeStrategyMethod, token, uint8(types.FeeStrategyBurn))
	h.call(trader, launchpad.BuyMethod, token, big.NewInt(333_333_334), big.NewInt(0), trader, deadline)
	require.Equal(t, big.NewInt(9_000_000), h.call(h.caller, launchpad.ExecuteBurnMethod, token)[0])
	require.Len(t, h.logs("FeesBurned(address,uint256)"), 1)

	require.Contains(t, h.reverts(stranger, launchpad.PauseMethod, token), "guardian")
	require.Equal(t, true, h.call(h.caller, launchpad.PauseMethod, token)[0])
	require.True(t, h.market(token).Paused)
	require.Contains(t, h.reverts(trader, launchpad.BuyMethod, token, big.NewInt(1_000_000), big.NewInt(0), trader, deadline), "paused")
	require.Contains(t, h.reverts(h.caller, launchpad.PauseMethod, token), "paused")
	h.call(h.caller, launchpad.UnpauseMethod, token)
	require.Len(t, h.logs("PauseToggled(address,bool)"), 2)
	h.call(trader, launchpad.BuyMethod, token, big.NewInt(1_000_000), big.NewInt(0), trader, deadline)
	message, broken := launchpadkeeper.SolvencyInvariant(h.keeper)(h.stateDB.Ctx())
	require.False(t, broken, message)
}

func TestRejectsDelegatecallStaticcallWritesAndValue(t *testing.T) {
	h := newHarness(t)
	token, _ := h.create()
	_, reason := h.run(h.caller, launchpad.BuyMethod, nil, false, true, token, big.NewInt(1_000_000), big.NewInt(0), h.caller, deadline)
	require.Contains(t, reason, "delegatecall")
	_, reason = h.run(h.caller, launchpad.GetMarketCountMethod, nil, true, true)
	require.Contains(t, reason, "delegatecall")
	_, reason = h.run(h.caller, launchpad.BuyMethod, nil, true, false, token, big.NewInt(1_000_000), big.NewInt(0), h.caller, deadline)
	require.Contains(t, reason, "staticcall")
	_, reason = h.run(h.caller, launchpad.CreateMarketMethod, nil, true, false, "Kindle Token", "KNDL", uint8(0))
	require.Contains(t, reason, "staticcall")
	_, reason = h.run(h.caller, launchpad.PauseMethod, nil, true, false, token)
	require.Contains(t, reason, "staticcall")
	_, reason = h.run(h.caller, launchpad.BuyMethod, big.NewInt(1), false, false, token, big.NewInt(1_000_000), big.NewInt(0), h.caller, deadline)
	require.NotEmpty(t, reason)
	require.Equal(t, big.NewInt(1), h.view(launchpad.GetMarketCountMethod)[0])
	require.True(t, h.market(token).RealQuoteBalance.Sign() == 0, "no rejected call moved funds")
}
