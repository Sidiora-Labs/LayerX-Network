package keeper_test

import (
	"encoding/json"
	"math/big"
	"testing"
	"time"

	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/modules/launchpad/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/launchpad/launchpadtest"
	"github.com/sidiora-labs/paxeer-network/modules/launchpad/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	authtypes "github.com/sidiora-labs/paxeer-network/sdk/x/auth/types"
	govtypes "github.com/sidiora-labs/paxeer-network/sdk/x/gov/types"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

const quote = types.DefaultQuoteDenom

var (
	genesisTime = time.Unix(1_800_000_000, 0).UTC()
	farDeadline = uint64(genesisTime.Add(10_000 * time.Hour).Unix())
	mature      = 1000 * time.Hour
)

// env is the launchpad keeper on the real bank, tokenfactory and EVM keepers
// of the test application, on a branch of its state.
type env struct {
	t   *testing.T
	ctx sdk.Context
	k   *keeper.Keeper
}

func newEnv(t *testing.T) *env {
	t.Helper()
	app := testkeeper.EVMTestApp
	ctx := app.GetContextForDeliverTx([]byte{}).WithBlockTime(genesisTime)
	ctx = ctx.WithGasMeter(sdk.NewInfiniteGasMeterWithMultiplier(ctx))
	k, ctx := launchpadtest.NewKeeper(app, ctx)
	k.InitGenesis(ctx, *types.DefaultGenesis())
	return &env{t: t, ctx: ctx, k: k}
}

func (e *env) account(quoteAmount int64) sdk.AccAddress {
	e.t.Helper()
	acc, _ := testkeeper.MockAddressPair()
	if quoteAmount > 0 {
		coins := sdk.NewCoins(sdk.NewCoin(quote, sdk.NewInt(quoteAmount)))
		require.NoError(e.t, testkeeper.EVMTestApp.BankKeeper.MintCoins(e.ctx, "evm", coins))
		require.NoError(e.t, testkeeper.EVMTestApp.BankKeeper.SendCoinsFromModuleToAccount(e.ctx, "evm", acc, coins))
	}
	return acc
}

func (e *env) balance(acc sdk.AccAddress, denom string) sdk.Int {
	return testkeeper.EVMTestApp.BankKeeper.GetBalance(e.ctx, acc, denom).Amount
}

func (e *env) at(d time.Duration) { e.ctx = e.ctx.WithBlockTime(genesisTime.Add(d)) }

func (e *env) create(creator sdk.AccAddress, strategy types.FeeStrategy) types.Market {
	e.t.Helper()
	market, err := e.k.CreateMarket(e.ctx, nil, creator, "Kindle Token", "KNDL", strategy)
	require.NoError(e.t, err)
	return market
}

func (e *env) market(denom string) types.Market {
	e.t.Helper()
	market, found := e.k.GetMarket(e.ctx, denom)
	require.True(e.t, found)
	return market
}

func (e *env) swap(trader sdk.AccAddress, denom string, isBuy bool, amountIn int64) keeper.SwapResult {
	e.t.Helper()
	result, err := e.k.Swap(e.ctx, trader, denom, isBuy, big.NewInt(amountIn), big.NewInt(0), trader, farDeadline)
	require.NoError(e.t, err)
	return result
}

func (e *env) solvent() {
	e.t.Helper()
	message, broken := keeper.SolvencyInvariant(e.k)(e.ctx)
	require.False(e.t, broken, message)
}

// sameJSON compares values by their stored encoding, where a zero amount
// has one form whatever produced it.
func sameJSON(t *testing.T, expected, actual interface{}) {
	t.Helper()
	want, err := json.Marshal(expected)
	require.NoError(t, err)
	got, err := json.Marshal(actual)
	require.NoError(t, err)
	require.JSONEq(t, string(want), string(got))
}

func i(value int64) sdk.Int { return sdk.NewInt(value) }

func b(value int64) *big.Int { return big.NewInt(value) }

func TestCreateMarketChargesFeeAndEscrowsSupply(t *testing.T) {
	e := newEnv(t)
	creator := e.account(250_000_000)
	market := e.create(creator, types.FeeStrategyClaim)

	require.Equal(t, i(100_000_000), e.balance(e.k.TreasuryAddress(), quote))
	require.Equal(t, i(150_000_000), e.balance(creator, quote))
	require.Equal(t, "factory/"+e.k.ModuleAddress().String()+"/lp1", market.Denom)
	require.Equal(t, i(1_000_000_000_000_000), e.balance(e.k.ModuleAddress(), market.Denom))
	require.Equal(t, i(1_000_000_000_000_000), testkeeper.EVMTestApp.BankKeeper.GetSupply(e.ctx, market.Denom).Amount)

	pointer, _, exists := testkeeper.EVMTestApp.EvmKeeper.GetERC20NativePointer(e.ctx, market.Denom)
	require.True(t, exists)
	require.Equal(t, pointer.Hex(), market.Pointer)
	byPointer, found := e.k.GetMarketByPointer(e.ctx, pointer)
	require.True(t, found)
	sameJSON(t, market, byPointer)
	metadata, found := testkeeper.EVMTestApp.BankKeeper.GetDenomMetaData(e.ctx, market.Denom)
	require.True(t, found)
	require.Equal(t, "KNDL", metadata.Symbol)
	require.Equal(t, uint32(types.TokenDecimals), metadata.DenomUnits[1].Exponent)

	require.Equal(t, creator.String(), market.Creator)
	require.Equal(t, creator.String(), market.Guardian)
	require.Equal(t, creator.String(), market.FeeRightsHolder)
	require.Equal(t, i(10_000_000_000), market.VirtualQuoteReserve)
	require.True(t, market.RealQuoteBalance.IsZero())
	require.Equal(t, market.TotalSupply, market.TokenReserve)
	require.Equal(t, genesisTime.Unix(), market.CreationTime)
	price, err := market.Price()
	require.NoError(t, err)
	require.Equal(t, b(10_000_000_000_000), price)

	second := e.create(creator, types.FeeStrategyBurn)
	require.Equal(t, uint64(2), e.k.GetMarketCount(e.ctx))
	sameJSON(t, []types.Market{market, second}, e.k.GetMarketsByCreator(e.ctx, creator, 0, 10))
	sameJSON(t, []types.Market{second}, e.k.GetMarkets(e.ctx, 1, 10))
	sameJSON(t, []types.Market{market}, e.k.GetMarkets(e.ctx, 0, 1))
	e.solvent()

	_, err = e.k.CreateMarket(e.ctx, nil, creator, "Kindle Token", "KNDL", types.FeeStrategyClaim)
	require.Error(t, err, "creator can no longer pay the creation fee")
	rich := e.account(1_000_000_000)
	_, err = e.k.CreateMarket(e.ctx, nil, rich, "Kindle Token", "KNDL", types.FeeStrategy(4))
	require.ErrorIs(t, err, types.ErrInvalidStrategy)
	_, err = e.k.CreateMarket(e.ctx, nil, rich, " ", "KNDL", types.FeeStrategyClaim)
	require.ErrorIs(t, err, types.ErrInvalidMarket)
}

// TestSwapVectors replays SidioraPool.swap on the protocol defaults and
// checks every output against the Solidity arithmetic.
func TestSwapVectors(t *testing.T) {
	e := newEnv(t)
	creator := e.account(100_000_000)
	alice := e.account(2_000_000_000)
	denom := e.create(creator, types.FeeStrategyClaim).Denom

	quoted, err := e.k.QuoteBuy(e.ctx, denom, b(100_000_000))
	require.NoError(t, err)
	buy := e.swap(alice, denom, true, 100_000_000)
	require.Equal(t, quoted, buy)
	require.Equal(t, b(300), buy.FeeBps)
	require.Equal(t, b(3_000_000), buy.FeeAmount)
	require.Equal(t, b(9_606_813_905_120), buy.AmountOut)
	require.Equal(t, b(300_000), buy.ProtocolCut)
	require.Equal(t, b(2_700_000), buy.PoolCut)
	m := e.market(denom)
	require.Equal(t, i(97_000_000), m.RealQuoteBalance)
	require.Equal(t, i(990_393_186_094_880), m.TokenReserve)
	require.Equal(t, i(100_000_000), m.CumulativeVolume)
	require.Equal(t, i(3_000_000), m.AccumulatedQuoteFees)
	require.Equal(t, i(2_700_000), m.AccumulatedFees)
	require.Equal(t, i(10_000_000_000), m.VirtualQuoteReserve)
	require.Equal(t, i(10_194_940_899_999), m.PriceSnapshots[0])
	require.Equal(t, uint64(1), m.SnapshotIndex)
	require.Equal(t, uint64(1), m.SnapshotCount)
	require.Equal(t, i(9_606_813_905_120), e.balance(alice, denom))
	require.Equal(t, i(100_300_000), e.balance(e.k.TreasuryAddress(), quote))
	require.Equal(t, i(300_000), e.k.GetProtocolFeesPending(e.ctx))

	tokenReserveBefore := m.TokenReserve
	sell := e.swap(alice, denom, false, 4_803_406_952_560)
	require.Equal(t, b(300), sell.FeeBps)
	require.Equal(t, b(144_102_208_576), sell.FeeAmount)
	require.Equal(t, b(47_278_912), sell.AmountOut)
	require.Equal(t, 0, sell.ProtocolCut.Sign())
	m = e.market(denom)
	require.Equal(t, tokenReserveBefore.Add(i(4_803_406_952_560)), m.TokenReserve)
	require.Equal(t, i(49_721_088), m.RealQuoteBalance)
	require.Equal(t, i(2_700_000), m.AccumulatedFees, "a sell does not touch the quote fees")
	require.Equal(t, i(144_102_208_576), m.AccumulatedTokenFees)
	require.Equal(t, i(10_098_226_981_692), m.PriceSnapshots[1])

	e.at(mature)
	fee, err := e.k.FeeBps(e.ctx, m)
	require.NoError(t, err)
	require.Equal(t, b(30), fee, "two snapshots have one change and no deviation")
	second := e.swap(alice, denom, true, 1_000_000_000)
	require.Equal(t, b(30), second.FeeBps)
	require.Equal(t, b(3_000_000), second.FeeAmount)
	require.Equal(t, b(89_819_503_485_620), second.AmountOut)
	m = e.market(denom)
	require.Equal(t, i(1_046_721_088), m.RealQuoteBalance)
	require.Equal(t, i(12_201_237_711_179), m.PriceSnapshots[2])

	last := e.swap(alice, denom, false, 89_819_503_485_620)
	require.Equal(t, b(300), last.FeeBps, "volatility of three snapshots lifts the fee to the cap")
	require.Equal(t, b(969_715_592), last.AmountOut)
	m = e.market(denom)
	require.Equal(t, i(77_005_496), m.RealQuoteBalance)
	require.Equal(t, i(995_196_593_047_440), m.TokenReserve)
	require.Equal(t, i(10_125_643_080_371), m.PriceSnapshots[3])
	require.Equal(t, uint64(4), m.SnapshotCount)
	require.Equal(t, i(100_000_000+4_803_406_952_560+1_000_000_000+89_819_503_485_620), m.CumulativeVolume)
	e.solvent()
}

func TestSnapshotRingWraps(t *testing.T) {
	e := newEnv(t)
	creator := e.account(100_000_000)
	alice := e.account(1_000_000_000)
	denom := e.create(creator, types.FeeStrategyClaim).Denom
	for n := 0; n < types.SnapshotSlots+2; n++ {
		e.swap(alice, denom, true, 1_000_000)
	}
	m := e.market(denom)
	require.Equal(t, uint64(types.SnapshotSlots), m.SnapshotCount)
	require.Equal(t, uint64(2), m.SnapshotIndex)
	price, err := m.Price()
	require.NoError(t, err)
	require.Equal(t, sdk.NewIntFromBigInt(price), m.PriceSnapshots[1])
}

// TestQuoteConservation checks, after every swap of a buy/sell sequence by
// two traders across fee regimes, that quote paid in equals quote paid out
// plus fees plus the real balance left in the curve, and that bank balances
// match the market's accounting.
func TestQuoteConservation(t *testing.T) {
	e := newEnv(t)
	creator := e.account(100_000_000)
	alice := e.account(5_000_000_000)
	bob := e.account(5_000_000_000)
	denom := e.create(creator, types.FeeStrategyClaim).Denom
	treasuryStart := e.balance(e.k.TreasuryAddress(), quote)

	quoteIn, quoteOut, quoteFees, protocol := sdk.ZeroInt(), sdk.ZeroInt(), sdk.ZeroInt(), sdk.ZeroInt()
	steps := []struct {
		at     time.Duration
		trader sdk.AccAddress
		isBuy  bool
		amount int64
	}{
		{0, alice, true, 250_000_000},
		{time.Minute, bob, true, 1_750_000_000},
		{time.Hour, alice, false, 0},
		{2 * time.Hour, bob, false, 0},
		{30 * time.Hour, alice, true, 999_999_999},
		{mature, bob, true, 12_345_678},
		{mature + time.Hour, bob, false, 0},
		{mature + 2*time.Hour, alice, false, 0},
	}
	for n, step := range steps {
		e.at(step.at)
		amount := step.amount
		if !step.isBuy {
			amount = e.balance(step.trader, denom).QuoRaw(3).Int64()
		}
		quoteBefore := e.balance(step.trader, quote)
		result := e.swap(step.trader, denom, step.isBuy, amount)
		if step.isBuy {
			quoteIn = quoteIn.AddRaw(amount)
			quoteFees = quoteFees.Add(sdk.NewIntFromBigInt(result.FeeAmount))
			protocol = protocol.Add(sdk.NewIntFromBigInt(result.ProtocolCut))
			require.Equal(t, quoteBefore.SubRaw(amount), e.balance(step.trader, quote), "step %d", n)
		} else {
			quoteOut = quoteOut.Add(sdk.NewIntFromBigInt(result.AmountOut))
			require.Equal(t, quoteBefore.Add(sdk.NewIntFromBigInt(result.AmountOut)), e.balance(step.trader, quote), "step %d", n)
		}
		m := e.market(denom)
		require.Equal(t, quoteIn, quoteOut.Add(quoteFees).Add(m.RealQuoteBalance), "step %d", n)
		require.Equal(t, quoteFees, m.AccumulatedQuoteFees, "step %d", n)
		require.Equal(t, quoteFees, m.AccumulatedFees.Add(protocol), "step %d", n)
		require.Equal(t, m.RealQuoteBalance.Add(m.AccumulatedFees), e.balance(e.k.ModuleAddress(), quote), "step %d", n)
		require.Equal(t, treasuryStart.Add(protocol), e.balance(e.k.TreasuryAddress(), quote), "step %d", n)
		require.Equal(t, m.TokenReserve, e.balance(e.k.ModuleAddress(), denom), "step %d", n)
		require.Equal(t, m.TotalSupply, m.TokenReserve.Add(e.balance(alice, denom)).Add(e.balance(bob, denom)), "step %d", n)
		require.Equal(t, i(10_000_000_000), m.VirtualQuoteReserve)
		e.solvent()
	}
}

func TestSwapRejections(t *testing.T) {
	e := newEnv(t)
	creator := e.account(100_000_000)
	alice := e.account(1_000_000_000)
	denom := e.create(creator, types.FeeStrategyClaim).Denom
	e.at(time.Hour)
	swap := func(isBuy bool, amount, minOut int64, recipient sdk.AccAddress, deadline uint64) error {
		_, err := e.k.Swap(e.ctx, alice, denom, isBuy, b(amount), b(minOut), recipient, deadline)
		return err
	}
	now := uint64(genesisTime.Add(time.Hour).Unix())
	require.ErrorIs(t, swap(true, 1_000_000, 0, alice, now-1), types.ErrDeadlineExpired)
	require.NoError(t, swap(true, 1_000_000, 0, alice, now), "the deadline block itself is allowed")
	require.ErrorIs(t, swap(true, 0, 0, alice, now), types.ErrInsufficientInput)
	require.ErrorIs(t, swap(true, 1_000_000, 0, nil, now), types.ErrZeroAddress)
	require.ErrorIs(t, swap(true, 1_000_000, 1_000_000_000_000_000, alice, now), types.ErrSlippageExceeded)
	require.ErrorIs(t, swap(false, 1, 1_000_000_000, alice, now), types.ErrSlippageExceeded)
	require.Error(t, swap(true, 2_000_000_000, 0, alice, now), "more quote than alice holds")
	_, err := e.k.Swap(e.ctx, alice, "factory/nobody/lp9", true, b(1), b(0), alice, now)
	require.ErrorIs(t, err, types.ErrUnknownMarket)

	// The virtual floor: a sell may never pay out virtual quote. Genesis
	// state with no real quote left behind the tokens alice holds.
	gs := e.k.ExportGenesis(e.ctx)
	gs.Markets[0].RealQuoteBalance = sdk.ZeroInt()
	e.k.InitGenesis(e.ctx, *gs)
	require.ErrorIs(t, swap(false, e.balance(alice, denom).Int64(), 0, alice, now), types.ErrVirtualFloorBreached)
}

func TestFeeSplitMatchesFeeAccumulator(t *testing.T) {
	e := newEnv(t)
	creator := e.account(100_000_000)
	alice := e.account(1_000_000_000)
	denom := e.create(creator, types.FeeStrategyClaim).Denom
	treasury := e.balance(e.k.TreasuryAddress(), quote)
	result := e.swap(alice, denom, true, 333_333_334)
	require.Equal(t, b(10_000_000), result.FeeAmount)
	require.Equal(t, b(1_000_000), result.ProtocolCut)
	require.Equal(t, b(9_000_000), result.PoolCut)
	require.Equal(t, treasury.AddRaw(1_000_000), e.balance(e.k.TreasuryAddress(), quote))
	require.Equal(t, i(9_000_000), e.market(denom).AccumulatedFees)
	require.Equal(t, i(1_000_000), e.k.GetProtocolFeesPending(e.ctx))
}

func TestFeeStrategies(t *testing.T) {
	e := newEnv(t)
	creator := e.account(100_000_000)
	alice := e.account(2_000_000_000)
	stranger := e.account(0)
	denom := e.create(creator, types.FeeStrategyClaim).Denom

	_, err := e.k.ClaimFees(e.ctx, creator, denom, creator)
	require.ErrorIs(t, err, types.ErrNoFeesAccumulated)
	e.swap(alice, denom, true, 333_333_334)

	_, err = e.k.ClaimFees(e.ctx, stranger, denom, stranger)
	require.ErrorIs(t, err, types.ErrNotFeeRightsHolder)
	_, err = e.k.ExecuteBurn(e.ctx, creator, denom)
	require.ErrorIs(t, err, types.ErrWrongStrategy)
	_, err = e.k.ClaimFees(e.ctx, creator, denom, nil)
	require.ErrorIs(t, err, types.ErrZeroAddress)
	claimed, err := e.k.ClaimFees(e.ctx, creator, denom, stranger)
	require.NoError(t, err)
	require.Equal(t, i(9_000_000), claimed)
	require.Equal(t, i(9_000_000), e.balance(stranger, quote))
	require.True(t, e.market(denom).AccumulatedFees.IsZero())
	_, err = e.k.ClaimFees(e.ctx, creator, denom, creator)
	require.ErrorIs(t, err, types.ErrNoFeesAccumulated)
	e.solvent()

	require.ErrorIs(t, e.k.SetFeeStrategy(e.ctx, stranger, denom, types.FeeStrategyBurn), types.ErrNotFeeRightsHolder)
	require.ErrorIs(t, e.k.SetFeeStrategy(e.ctx, creator, denom, types.FeeStrategy(4)), types.ErrInvalidStrategy)
	require.NoError(t, e.k.SetFeeStrategy(e.ctx, creator, denom, types.FeeStrategyBurn))
	e.swap(alice, denom, true, 333_333_334)
	pool := e.market(denom).AccumulatedFees
	dead := e.k.DeadAccount(e.ctx)
	require.Equal(t, testkeeper.EVMTestApp.EvmKeeper.GetPaxAddressOrDefault(e.ctx, common.HexToAddress(types.DeadAddress)), dead)
	deadBefore := e.balance(dead, quote)
	burned, err := e.k.ExecuteBurn(e.ctx, creator, denom)
	require.NoError(t, err)
	require.Equal(t, pool, burned)
	require.Equal(t, deadBefore.Add(pool), e.balance(dead, quote))
	e.solvent()

	require.NoError(t, e.k.SetFeeStrategy(e.ctx, creator, denom, types.FeeStrategyLpRewards))
	e.swap(alice, denom, true, 333_333_334)
	before := e.market(denom)
	escrowBefore := e.balance(e.k.ModuleAddress(), quote)
	rewards, err := e.k.ExecuteLpRewards(e.ctx, creator, denom)
	require.NoError(t, err)
	require.Equal(t, before.AccumulatedFees, rewards)
	after := e.market(denom)
	require.Equal(t, before.RealQuoteBalance.Add(rewards), after.RealQuoteBalance)
	require.True(t, after.AccumulatedFees.IsZero())
	require.Equal(t, escrowBefore, e.balance(e.k.ModuleAddress(), quote), "LP rewards stay in escrow")
	_, err = e.k.ExecuteLpRewards(e.ctx, creator, denom)
	require.ErrorIs(t, err, types.ErrNoFeesAccumulated)
	e.solvent()
}

// TestAirdropSharesByHolding reproduces FeeAccumulator's airdrop vector: a
// 10 USDL fee leaves 9 USDL for holders and a 60% holder takes 5.4 USDL.
func TestAirdropSharesByHolding(t *testing.T) {
	e := newEnv(t)
	gov := authtypes.NewModuleAddress(govtypes.ModuleName).String()
	params := types.DefaultParams()
	params.VirtualQuoteDefault = i(1_000_000_000)
	params.VirtualTokenDefault = i(1_000_000_000)
	require.NoError(t, e.k.UpdateParams(e.ctx, gov, params))
	creator := e.account(100_000_000)
	alice := e.account(4_000_000_000)
	bob := e.account(0)
	carol := e.account(0)
	denom := e.create(creator, types.FeeStrategyAirdrop).Denom

	_, err := e.k.ClaimAirdrop(e.ctx, alice, denom)
	require.ErrorIs(t, err, types.ErrAirdropNotTriggered)
	e.at(mature)
	result := e.swap(alice, denom, true, 3_333_333_334)
	require.Equal(t, b(30), result.FeeBps)
	require.Equal(t, b(10_000_000), result.FeeAmount)
	require.Equal(t, b(768_696_993), result.AmountOut)
	require.NoError(t, testkeeper.EVMTestApp.BankKeeper.SendCoins(e.ctx, alice, bob,
		sdk.NewCoins(sdk.NewCoin(denom, i(168_696_993)))))
	require.Equal(t, i(600_000_000), e.balance(alice, denom))

	_, err = e.k.ExecuteAirdrop(e.ctx, bob, denom)
	require.ErrorIs(t, err, types.ErrNotFeeRightsHolder)
	amount, err := e.k.ExecuteAirdrop(e.ctx, creator, denom)
	require.NoError(t, err)
	require.Equal(t, i(9_000_000), amount)
	m := e.market(denom)
	require.Equal(t, uint64(1), m.AirdropEpoch)
	require.Equal(t, i(9_000_000), m.AirdropBalance)
	require.Equal(t, i(9_000_000), e.k.GetAirdropEpochAmount(e.ctx, denom, 1))
	_, err = e.k.ExecuteAirdrop(e.ctx, creator, denom)
	require.ErrorIs(t, err, types.ErrNoFeesAccumulated)

	aliceBefore := e.balance(alice, quote)
	share, err := e.k.ClaimAirdrop(e.ctx, alice, denom)
	require.NoError(t, err)
	require.Equal(t, i(5_400_000), share)
	require.Equal(t, aliceBefore.AddRaw(5_400_000), e.balance(alice, quote))
	require.True(t, e.k.HasClaimedAirdrop(e.ctx, denom, alice, 1))
	_, err = e.k.ClaimAirdrop(e.ctx, alice, denom)
	require.ErrorIs(t, err, types.ErrAlreadyClaimed)
	bobShare, err := e.k.ClaimAirdrop(e.ctx, bob, denom)
	require.NoError(t, err)
	require.Equal(t, i(1_518_272), bobShare)
	_, err = e.k.ClaimAirdrop(e.ctx, carol, denom)
	require.ErrorIs(t, err, types.ErrZeroAmount)
	require.Equal(t, i(9_000_000-5_400_000-1_518_272), e.market(denom).AirdropBalance)
	e.solvent()

	gs := e.k.ExportGenesis(e.ctx)
	require.NoError(t, gs.Validate())
	require.Equal(t, []types.AirdropEpochAmount{{Denom: denom, Epoch: 1, Amount: i(9_000_000)}}, gs.AirdropEpochs)
	require.Len(t, gs.AirdropClaims, 2)
}

func TestGuardianPause(t *testing.T) {
	e := newEnv(t)
	creator := e.account(100_000_000)
	alice := e.account(1_000_000_000)
	denom := e.create(creator, types.FeeStrategyClaim).Denom
	require.ErrorIs(t, e.k.Pause(e.ctx, alice, denom), types.ErrNotGuardian)
	require.ErrorIs(t, e.k.Unpause(e.ctx, creator, denom), types.ErrNotPaused)
	require.NoError(t, e.k.Pause(e.ctx, creator, denom))
	require.ErrorIs(t, e.k.Pause(e.ctx, creator, denom), types.ErrPaused)
	_, err := e.k.Swap(e.ctx, alice, denom, true, b(1_000_000), b(0), alice, farDeadline)
	require.ErrorIs(t, err, types.ErrPaused)
	require.ErrorIs(t, e.k.Unpause(e.ctx, alice, denom), types.ErrNotGuardian)
	require.NoError(t, e.k.Unpause(e.ctx, creator, denom))
	e.swap(alice, denom, true, 1_000_000)
}

func TestParamsChangeOnlyThroughGovernance(t *testing.T) {
	e := newEnv(t)
	gov := authtypes.NewModuleAddress(govtypes.ModuleName).String()
	require.Equal(t, gov, e.k.Authority())
	params := types.DefaultParams()
	params.ProtocolFeeBps = 2_000
	params.CreationFee = i(5)
	outsider, _ := testkeeper.MockAddressPair()
	require.ErrorIs(t, e.k.UpdateParams(e.ctx, outsider.String(), params), types.ErrUnauthorized)
	sameJSON(t, types.DefaultParams(), e.k.GetParams(e.ctx))
	invalid := params
	invalid.MaxFeeBps = 10_000
	require.ErrorIs(t, e.k.UpdateParams(e.ctx, gov, invalid), types.ErrInvalidParams)
	require.NoError(t, e.k.UpdateParams(e.ctx, gov, params))
	sameJSON(t, params, e.k.GetParams(e.ctx))

	creator := e.account(5)
	alice := e.account(1_000_000_000)
	denom := e.create(creator, types.FeeStrategyClaim).Denom
	require.Equal(t, i(5), e.balance(e.k.TreasuryAddress(), quote))
	result := e.swap(alice, denom, true, 333_333_334)
	require.Equal(t, b(2_000_000), result.ProtocolCut)
	require.Equal(t, b(8_000_000), result.PoolCut)
}

func TestGenesisRoundTrip(t *testing.T) {
	e := newEnv(t)
	creator := e.account(200_000_000)
	alice := e.account(1_000_000_000)
	first := e.create(creator, types.FeeStrategyClaim).Denom
	e.create(creator, types.FeeStrategyLpRewards)
	e.swap(alice, first, true, 50_000_000)
	e.swap(alice, first, false, e.balance(alice, first).QuoRaw(2).Int64())
	require.NoError(t, e.k.Pause(e.ctx, creator, first))
	exported := e.k.ExportGenesis(e.ctx)
	require.NoError(t, exported.Validate())
	require.Len(t, exported.Markets, 2)

	fresh, ctx := launchpadtest.NewKeeper(testkeeper.EVMTestApp, e.ctx)
	fresh.InitGenesis(ctx, *exported)
	sameJSON(t, exported, fresh.ExportGenesis(ctx))
	sameJSON(t, e.k.GetMarkets(e.ctx, 0, 10), fresh.GetMarkets(ctx, 0, 10))
	pointer := common.HexToAddress(exported.Markets[0].Pointer)
	market, found := fresh.GetMarketByPointer(ctx, pointer)
	require.True(t, found)
	require.True(t, market.Paused)

	broken := *exported
	broken.Markets = []types.Market{exported.Markets[1]}
	require.ErrorIs(t, broken.Validate(), types.ErrInvalidGenesis)
}
