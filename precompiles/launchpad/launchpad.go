// Package launchpad is the precompile through which EVM accounts and
// contracts use the native launchpad module: creating bonding-curve markets
// over tokenfactory denoms, buying and selling on them, managing the fee
// rights of a market and reading market state.
//
// A market is addressed by its token's ERC20 pointer. Quote and tokens move
// between the bank accounts of the caller, the recipient and the launchpad
// module account; the precompile never holds funds.
//
// Every state change is emitted both as an EVM log from this address and as
// a Cosmos event.
//
// Gas is charged by the EVM from RequiredGas before Execute runs:
//
//	gas = BaseGas
//	    + GasPerByte    * len(calldata after the selector)
//	    + GasPerWrite   * writes(method)
//	    + GasPerPointer * pointers(method)
//
// writes is the fixed number of state slots a method may touch: createMarket
// 12, buy 8, sell 6, claimFees and executeBurn 3, executeAirdrop and
// claimAirdrop 4, setFeeStrategy, executeLpRewards, pause and unpause 1,
// views 0. pointers is 1 for createMarket, which deploys the token's ERC20
// pointer, and 0 otherwise.
package launchpad

import (
	"embed"
	"errors"
	"fmt"
	"math"
	"math/big"

	"github.com/ethereum/go-ethereum/accounts/abi"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/tracing"
	"github.com/ethereum/go-ethereum/core/vm"
	launchpadkeeper "github.com/sidiora-labs/paxeer-network/modules/launchpad/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/launchpad/types"
	pcommon "github.com/sidiora-labs/paxeer-network/precompiles/common"
	"github.com/sidiora-labs/paxeer-network/precompiles/utils"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

const (
	CreateMarketMethod        = "createMarket"
	BuyMethod                 = "buy"
	SellMethod                = "sell"
	SetFeeStrategyMethod      = "setFeeStrategy"
	ClaimFeesMethod           = "claimFees"
	ExecuteBurnMethod         = "executeBurn"
	ExecuteAirdropMethod      = "executeAirdrop"
	ClaimAirdropMethod        = "claimAirdrop"
	ExecuteLpRewardsMethod    = "executeLpRewards"
	PauseMethod               = "pause"
	UnpauseMethod             = "unpause"
	QuoteBuyMethod            = "quoteBuy"
	QuoteSellMethod           = "quoteSell"
	GetReservesMethod         = "getReserves"
	GetPriceMethod            = "getPrice"
	GetPriceSnapshotsMethod   = "getPriceSnapshots"
	GetFeeBpsMethod           = "getFeeBps"
	GetMarketMethod           = "getMarket"
	GetMarketsMethod          = "getMarkets"
	GetMarketsByCreatorMethod = "getMarketsByCreator"
	GetMarketCountMethod      = "getMarketCount"
	GetAccumulatedFeesMethod  = "getAccumulatedFees"
	GetConfigMethod           = "getConfig"

	MarketCreatedEvent      = "MarketCreated"
	SwapEvent               = "Swap"
	FeeRecordedEvent        = "FeeRecorded"
	FeeStrategyChangedEvent = "FeeStrategyChanged"
	FeesClaimedEvent        = "FeesClaimed"
	FeesBurnedEvent         = "FeesBurned"
	AirdropExecutedEvent    = "AirdropExecuted"
	AirdropClaimedEvent     = "AirdropClaimed"
	LpRewardsExecutedEvent  = "LpRewardsExecuted"
	PauseToggledEvent       = "PauseToggled"
)

const (
	LaunchpadAddress = types.LaunchpadAddress
	PrecompileName   = "launchpad"

	BaseGas       uint64 = 3000
	GasPerByte    uint64 = 16
	GasPerWrite   uint64 = 5000
	GasPerPointer uint64 = 9_000_000
)

//go:embed abi.json
var f embed.FS

// Keeper is the launchpad module surface the precompile drives.
type Keeper interface {
	GetParams(ctx sdk.Context) types.Params
	GetProtocolFeesPending(ctx sdk.Context) sdk.Int
	GetMarketCount(ctx sdk.Context) uint64
	GetMarketByPointer(ctx sdk.Context, pointer common.Address) (types.Market, bool)
	GetMarkets(ctx sdk.Context, offset, limit uint64) []types.Market
	GetMarketsByCreator(ctx sdk.Context, creator sdk.AccAddress, offset, limit uint64) []types.Market
	FeeBps(ctx sdk.Context, market types.Market) (*big.Int, error)
	QuoteBuy(ctx sdk.Context, denom string, quoteIn *big.Int) (launchpadkeeper.SwapResult, error)
	QuoteSell(ctx sdk.Context, denom string, amountIn *big.Int) (launchpadkeeper.SwapResult, error)
	CreateMarket(ctx sdk.Context, evm *vm.EVM, creator sdk.AccAddress, name, symbol string, strategy types.FeeStrategy) (types.Market, error)
	Swap(ctx sdk.Context, trader sdk.AccAddress, denom string, isBuy bool, amountIn, minAmountOut *big.Int,
		recipient sdk.AccAddress, deadline uint64) (launchpadkeeper.SwapResult, error)
	SetFeeStrategy(ctx sdk.Context, caller sdk.AccAddress, denom string, strategy types.FeeStrategy) error
	ClaimFees(ctx sdk.Context, caller sdk.AccAddress, denom string, recipient sdk.AccAddress) (sdk.Int, error)
	ExecuteBurn(ctx sdk.Context, caller sdk.AccAddress, denom string) (sdk.Int, error)
	ExecuteAirdrop(ctx sdk.Context, caller sdk.AccAddress, denom string) (sdk.Int, error)
	ClaimAirdrop(ctx sdk.Context, holder sdk.AccAddress, denom string) (sdk.Int, error)
	ExecuteLpRewards(ctx sdk.Context, caller sdk.AccAddress, denom string) (sdk.Int, error)
	Pause(ctx sdk.Context, caller sdk.AccAddress, denom string) error
	Unpause(ctx sdk.Context, caller sdk.AccAddress, denom string) error
}

var _ Keeper = (*launchpadkeeper.Keeper)(nil)

// LaunchpadKeepers is the precompile keepers set once it carries the
// launchpad keeper.
type LaunchpadKeepers interface {
	LaunchpadK() *launchpadkeeper.Keeper
}

// Market is the ABI tuple of a market.
type Market struct {
	Token                common.Address
	Denom                string
	Index                uint64
	Name                 string
	Symbol               string
	Creator              common.Address
	Guardian             common.Address
	FeeRightsHolder      common.Address
	FeeStrategy          uint8
	Paused               bool
	TotalSupply          *big.Int
	VirtualQuoteReserve  *big.Int
	RealQuoteBalance     *big.Int
	TokenReserve         *big.Int
	CreatedAt            *big.Int
	CumulativeVolume     *big.Int
	AccumulatedQuoteFees *big.Int
	AccumulatedTokenFees *big.Int
	AccumulatedFees      *big.Int
	AirdropEpoch         *big.Int
	AirdropBalance       *big.Int
	Price                *big.Int
}

// Config is the ABI tuple of the launchpad params.
type Config struct {
	QuoteDenom          string
	VirtualQuoteDefault *big.Int
	VirtualTokenDefault *big.Int
	MinFeeBps           *big.Int
	MaxFeeBps           *big.Int
	BaseFeeBps          *big.Int
	ProtocolFeeBps      *big.Int
	FeeDecayRate        *big.Int
	VolatilityWeight    *big.Int
	ConcentrationWeight *big.Int
	CreationFee         *big.Int
	ProtocolFeesPending *big.Int
}

type PrecompileExecutor struct {
	abi       abi.ABI
	address   common.Address
	keeper    Keeper
	evmKeeper utils.EVMKeeper
}

// NewPrecompile builds the precompile from a keepers set that provides
// LaunchpadK.
func NewPrecompile(keepers utils.Keepers) (*pcommon.Precompile, error) {
	k := keepers.LaunchpadK()
	if k == nil {
		return nil, errors.New("launchpad: keepers do not provide the launchpad keeper")
	}
	return NewPrecompileWithKeeper(k, keepers.EVMK())
}

// NewPrecompileWithKeeper builds the precompile over k.
func NewPrecompileWithKeeper(k Keeper, evmKeeper utils.EVMKeeper) (*pcommon.Precompile, error) {
	newAbi := pcommon.MustGetABI(f, "abi.json")
	p := &PrecompileExecutor{abi: newAbi, address: common.HexToAddress(LaunchpadAddress), keeper: k, evmKeeper: evmKeeper}
	return pcommon.NewPrecompile(newAbi, p, p.address, PrecompileName).WithRevertReasons(), nil
}

// Writes returns the fixed state-slot bound of a method.
func Writes(method string) uint64 {
	switch method {
	case CreateMarketMethod:
		return 12
	case BuyMethod:
		return 8
	case SellMethod:
		return 6
	case ClaimFeesMethod, ExecuteBurnMethod:
		return 3
	case ExecuteAirdropMethod, ClaimAirdropMethod:
		return 4
	case SetFeeStrategyMethod, ExecuteLpRewardsMethod, PauseMethod, UnpauseMethod:
		return 1
	default:
		return 0
	}
}

// Pointers returns the ERC20 pointer deployments of a method.
func Pointers(method string) uint64 {
	if method == CreateMarketMethod {
		return 1
	}
	return 0
}

// Gas is the documented formula over already-measured quantities.
func Gas(inputBytes, writes, pointers uint64) uint64 {
	return BaseGas + GasPerByte*inputBytes + GasPerWrite*writes + GasPerPointer*pointers
}

func (p PrecompileExecutor) RequiredGas(input []byte, method *abi.Method) uint64 {
	return Gas(uint64(len(input)), Writes(method.Name), Pointers(method.Name))
}

func (p PrecompileExecutor) Execute(ctx sdk.Context, method *abi.Method, caller common.Address, _ common.Address, args []interface{}, value *big.Int, readOnly bool, evm *vm.EVM, _ *tracing.Hooks) (ret []byte, err error) {
	defer func() {
		if recovered := recover(); recovered != nil {
			ret = nil
			err = fmt.Errorf("launchpad: %v", recovered)
		}
	}()
	if ctx.EVMPrecompileCalledFromDelegateCall() {
		return nil, errors.New("cannot delegatecall launchpad")
	}
	if err = pcommon.ValidateNonPayable(value); err != nil {
		return nil, err
	}
	if readOnly && Writes(method.Name) != 0 {
		return nil, errors.New("cannot call a launchpad state change from staticcall")
	}
	switch method.Name {
	case CreateMarketMethod:
		return p.createMarket(ctx, method, caller, args, evm)
	case BuyMethod, SellMethod:
		return p.swap(ctx, method, caller, args, evm)
	case SetFeeStrategyMethod:
		return p.setFeeStrategy(ctx, method, caller, args, evm)
	case ClaimFeesMethod:
		return p.claimFees(ctx, method, caller, args, evm)
	case ExecuteBurnMethod, ExecuteAirdropMethod, ExecuteLpRewardsMethod:
		return p.executeStrategy(ctx, method, caller, args, evm)
	case ClaimAirdropMethod:
		return p.claimAirdrop(ctx, method, caller, args, evm)
	case PauseMethod, UnpauseMethod:
		return p.setPaused(ctx, method, caller, args, evm)
	case QuoteBuyMethod, QuoteSellMethod:
		return p.quote(ctx, method, args)
	case GetReservesMethod:
		market, err := p.market(ctx, args, 1)
		if err != nil {
			return nil, err
		}
		return method.Outputs.Pack(market.VirtualQuoteReserve.BigInt(), market.RealQuoteBalance.BigInt(),
			market.TokenReserve.BigInt())
	case GetPriceMethod:
		market, err := p.market(ctx, args, 1)
		if err != nil {
			return nil, err
		}
		price, err := market.Price()
		if err != nil {
			return nil, err
		}
		return method.Outputs.Pack(price)
	case GetPriceSnapshotsMethod:
		market, err := p.market(ctx, args, 1)
		if err != nil {
			return nil, err
		}
		return method.Outputs.Pack(market.Snapshots(), new(big.Int).SetUint64(market.SnapshotIndex),
			new(big.Int).SetUint64(market.SnapshotCount))
	case GetFeeBpsMethod:
		market, err := p.market(ctx, args, 1)
		if err != nil {
			return nil, err
		}
		fee, err := p.keeper.FeeBps(ctx, market)
		if err != nil {
			return nil, err
		}
		return method.Outputs.Pack(fee)
	case GetMarketMethod:
		market, err := p.market(ctx, args, 1)
		if err != nil {
			return nil, err
		}
		record, err := p.marketRecord(ctx, market)
		if err != nil {
			return nil, err
		}
		return method.Outputs.Pack(record)
	case GetMarketsMethod:
		if err = pcommon.ValidateArgsLength(args, 2); err != nil {
			return nil, err
		}
		return p.packMarkets(ctx, method, p.keeper.GetMarkets(ctx, clampUint64(args[0].(*big.Int)), clampUint64(args[1].(*big.Int))))
	case GetMarketsByCreatorMethod:
		if err = pcommon.ValidateArgsLength(args, 1); err != nil {
			return nil, err
		}
		creator := p.evmKeeper.GetPaxAddressOrDefault(ctx, args[0].(common.Address))
		return p.packMarkets(ctx, method, p.keeper.GetMarketsByCreator(ctx, creator, 0, math.MaxUint64))
	case GetMarketCountMethod:
		return method.Outputs.Pack(new(big.Int).SetUint64(p.keeper.GetMarketCount(ctx)))
	case GetAccumulatedFeesMethod:
		market, err := p.market(ctx, args, 1)
		if err != nil {
			return nil, err
		}
		return method.Outputs.Pack(market.AccumulatedFees.BigInt())
	case GetConfigMethod:
		return method.Outputs.Pack(p.config(ctx))
	}
	return nil, fmt.Errorf("launchpad: unknown method %s", method.Name)
}

func clampUint64(value *big.Int) uint64 {
	if !value.IsUint64() {
		return math.MaxUint64
	}
	return value.Uint64()
}

// account maps an EVM address to its bank account; the zero address maps to
// no account so the module rejects it as Solidity does.
func (p PrecompileExecutor) account(ctx sdk.Context, address common.Address) sdk.AccAddress {
	if address == (common.Address{}) {
		return nil
	}
	return p.evmKeeper.GetPaxAddressOrDefault(ctx, address)
}

func (p PrecompileExecutor) evmAddress(ctx sdk.Context, bech32 string) common.Address {
	account, err := sdk.AccAddressFromBech32(bech32)
	if err != nil {
		return common.Address{}
	}
	return p.evmKeeper.GetEVMAddressOrDefault(ctx, account)
}

// market resolves the token argument (always the first) of a call with
// wantArgs arguments.
func (p PrecompileExecutor) market(ctx sdk.Context, args []interface{}, wantArgs int) (types.Market, error) {
	if err := pcommon.ValidateArgsLength(args, wantArgs); err != nil {
		return types.Market{}, err
	}
	token := args[0].(common.Address)
	market, found := p.keeper.GetMarketByPointer(ctx, token)
	if !found {
		return types.Market{}, fmt.Errorf("%w: token %s", types.ErrUnknownMarket, token.Hex())
	}
	return market, nil
}

func (p PrecompileExecutor) marketRecord(ctx sdk.Context, m types.Market) (Market, error) {
	price, err := m.Price()
	if err != nil {
		return Market{}, err
	}
	return Market{Token: common.HexToAddress(m.Pointer), Denom: m.Denom, Index: m.Index, Name: m.Name, Symbol: m.Symbol,
		Creator: p.evmAddress(ctx, m.Creator), Guardian: p.evmAddress(ctx, m.Guardian),
		FeeRightsHolder: p.evmAddress(ctx, m.FeeRightsHolder), FeeStrategy: uint8(m.FeeStrategy), Paused: m.Paused,
		TotalSupply: m.TotalSupply.BigInt(), VirtualQuoteReserve: m.VirtualQuoteReserve.BigInt(),
		RealQuoteBalance: m.RealQuoteBalance.BigInt(), TokenReserve: m.TokenReserve.BigInt(),
		CreatedAt: big.NewInt(m.CreationTime), CumulativeVolume: m.CumulativeVolume.BigInt(),
		AccumulatedQuoteFees: m.AccumulatedQuoteFees.BigInt(), AccumulatedTokenFees: m.AccumulatedTokenFees.BigInt(),
		AccumulatedFees: m.AccumulatedFees.BigInt(), AirdropEpoch: new(big.Int).SetUint64(m.AirdropEpoch),
		AirdropBalance: m.AirdropBalance.BigInt(), Price: price}, nil
}

func (p PrecompileExecutor) packMarkets(ctx sdk.Context, method *abi.Method, markets []types.Market) ([]byte, error) {
	records := make([]Market, 0, len(markets))
	for _, market := range markets {
		record, err := p.marketRecord(ctx, market)
		if err != nil {
			return nil, err
		}
		records = append(records, record)
	}
	return method.Outputs.Pack(records)
}

func (p PrecompileExecutor) config(ctx sdk.Context) Config {
	params := p.keeper.GetParams(ctx)
	u := func(value uint64) *big.Int { return new(big.Int).SetUint64(value) }
	return Config{QuoteDenom: params.QuoteDenom, VirtualQuoteDefault: params.VirtualQuoteDefault.BigInt(),
		VirtualTokenDefault: params.VirtualTokenDefault.BigInt(), MinFeeBps: u(params.MinFeeBps),
		MaxFeeBps: u(params.MaxFeeBps), BaseFeeBps: u(params.BaseFeeBps), ProtocolFeeBps: u(params.ProtocolFeeBps),
		FeeDecayRate: u(params.FeeDecayRate), VolatilityWeight: u(params.VolatilityWeight),
		ConcentrationWeight: u(params.ConcentrationWeight), CreationFee: params.CreationFee.BigInt(),
		ProtocolFeesPending: p.keeper.GetProtocolFeesPending(ctx).BigInt()}
}

// log emits one ABI event from the launchpad address: indexed values become
// topics in declaration order and the rest is ABI-encoded data.
func (p PrecompileExecutor) log(evm *vm.EVM, name string, topics []common.Hash, data ...interface{}) error {
	event, ok := p.abi.Events[name]
	if !ok {
		return fmt.Errorf("launchpad: unknown event %s", name)
	}
	packed, err := event.Inputs.NonIndexed().Pack(data...)
	if err != nil {
		return err
	}
	return pcommon.EmitEVMLog(evm, p.address, append([]common.Hash{event.ID}, topics...), packed)
}

func addressTopic(value common.Address) common.Hash { return common.BytesToHash(value.Bytes()) }

func (p PrecompileExecutor) createMarket(ctx sdk.Context, method *abi.Method, caller common.Address, args []interface{},
	evm *vm.EVM) ([]byte, error) {
	if err := pcommon.ValidateArgsLength(args, 3); err != nil {
		return nil, err
	}
	strategy := types.FeeStrategy(args[2].(uint8))
	market, err := p.keeper.CreateMarket(ctx, evm, p.account(ctx, caller), args[0].(string), args[1].(string), strategy)
	if err != nil {
		return nil, err
	}
	token := common.HexToAddress(market.Pointer)
	if err := p.log(evm, MarketCreatedEvent, []common.Hash{addressTopic(token), addressTopic(caller)},
		market.Denom, market.Name, market.Symbol, uint8(strategy)); err != nil {
		return nil, err
	}
	return method.Outputs.Pack(token, market.Denom)
}

func (p PrecompileExecutor) swap(ctx sdk.Context, method *abi.Method, caller common.Address, args []interface{},
	evm *vm.EVM) ([]byte, error) {
	market, err := p.market(ctx, args, 5)
	if err != nil {
		return nil, err
	}
	isBuy := method.Name == BuyMethod
	amountIn, minOut := args[1].(*big.Int), args[2].(*big.Int)
	recipient := args[3].(common.Address)
	result, err := p.keeper.Swap(ctx, p.account(ctx, caller), market.Denom, isBuy, amountIn, minOut,
		p.account(ctx, recipient), clampUint64(args[4].(*big.Int)))
	if err != nil {
		return nil, err
	}
	token := args[0].(common.Address)
	updated, _ := p.keeper.GetMarketByPointer(ctx, token)
	price, err := updated.Price()
	if err != nil {
		return nil, err
	}
	if err := p.log(evm, SwapEvent, []common.Hash{addressTopic(token), addressTopic(caller), addressTopic(recipient)},
		isBuy, amountIn, result.AmountOut, result.FeeAmount, price); err != nil {
		return nil, err
	}
	if isBuy && result.FeeAmount.Sign() > 0 {
		if err := p.log(evm, FeeRecordedEvent, []common.Hash{addressTopic(token)},
			result.FeeAmount, result.ProtocolCut, result.PoolCut); err != nil {
			return nil, err
		}
	}
	return method.Outputs.Pack(result.AmountOut)
}

func (p PrecompileExecutor) setFeeStrategy(ctx sdk.Context, method *abi.Method, caller common.Address,
	args []interface{}, evm *vm.EVM) ([]byte, error) {
	market, err := p.market(ctx, args, 2)
	if err != nil {
		return nil, err
	}
	strategy := types.FeeStrategy(args[1].(uint8))
	if err := p.keeper.SetFeeStrategy(ctx, p.account(ctx, caller), market.Denom, strategy); err != nil {
		return nil, err
	}
	if err := p.log(evm, FeeStrategyChangedEvent, []common.Hash{addressTopic(args[0].(common.Address))},
		uint8(market.FeeStrategy), uint8(strategy)); err != nil {
		return nil, err
	}
	return method.Outputs.Pack(uint8(market.FeeStrategy))
}

func (p PrecompileExecutor) claimFees(ctx sdk.Context, method *abi.Method, caller common.Address, args []interface{},
	evm *vm.EVM) ([]byte, error) {
	market, err := p.market(ctx, args, 2)
	if err != nil {
		return nil, err
	}
	recipient := args[1].(common.Address)
	amount, err := p.keeper.ClaimFees(ctx, p.account(ctx, caller), market.Denom, p.account(ctx, recipient))
	if err != nil {
		return nil, err
	}
	if err := p.log(evm, FeesClaimedEvent, []common.Hash{addressTopic(args[0].(common.Address)), addressTopic(recipient)},
		amount.BigInt()); err != nil {
		return nil, err
	}
	return method.Outputs.Pack(amount.BigInt())
}

func (p PrecompileExecutor) executeStrategy(ctx sdk.Context, method *abi.Method, caller common.Address,
	args []interface{}, evm *vm.EVM) ([]byte, error) {
	market, err := p.market(ctx, args, 1)
	if err != nil {
		return nil, err
	}
	account := p.account(ctx, caller)
	topics := []common.Hash{addressTopic(args[0].(common.Address))}
	var amount sdk.Int
	switch method.Name {
	case ExecuteBurnMethod:
		if amount, err = p.keeper.ExecuteBurn(ctx, account, market.Denom); err == nil {
			err = p.log(evm, FeesBurnedEvent, topics, amount.BigInt())
		}
	case ExecuteAirdropMethod:
		if amount, err = p.keeper.ExecuteAirdrop(ctx, account, market.Denom); err == nil {
			err = p.log(evm, AirdropExecutedEvent, topics, amount.BigInt(), new(big.Int).SetUint64(market.AirdropEpoch+1))
		}
	default:
		if amount, err = p.keeper.ExecuteLpRewards(ctx, account, market.Denom); err == nil {
			err = p.log(evm, LpRewardsExecutedEvent, topics, amount.BigInt())
		}
	}
	if err != nil {
		return nil, err
	}
	return method.Outputs.Pack(amount.BigInt())
}

func (p PrecompileExecutor) claimAirdrop(ctx sdk.Context, method *abi.Method, caller common.Address,
	args []interface{}, evm *vm.EVM) ([]byte, error) {
	market, err := p.market(ctx, args, 1)
	if err != nil {
		return nil, err
	}
	amount, err := p.keeper.ClaimAirdrop(ctx, p.account(ctx, caller), market.Denom)
	if err != nil {
		return nil, err
	}
	if err := p.log(evm, AirdropClaimedEvent, []common.Hash{addressTopic(args[0].(common.Address)), addressTopic(caller)},
		amount.BigInt(), new(big.Int).SetUint64(market.AirdropEpoch)); err != nil {
		return nil, err
	}
	return method.Outputs.Pack(amount.BigInt())
}

func (p PrecompileExecutor) setPaused(ctx sdk.Context, method *abi.Method, caller common.Address, args []interface{},
	evm *vm.EVM) ([]byte, error) {
	market, err := p.market(ctx, args, 1)
	if err != nil {
		return nil, err
	}
	paused := method.Name == PauseMethod
	if paused {
		err = p.keeper.Pause(ctx, p.account(ctx, caller), market.Denom)
	} else {
		err = p.keeper.Unpause(ctx, p.account(ctx, caller), market.Denom)
	}
	if err != nil {
		return nil, err
	}
	if err := p.log(evm, PauseToggledEvent, []common.Hash{addressTopic(args[0].(common.Address))}, paused); err != nil {
		return nil, err
	}
	return method.Outputs.Pack(true)
}

func (p PrecompileExecutor) quote(ctx sdk.Context, method *abi.Method, args []interface{}) ([]byte, error) {
	market, err := p.market(ctx, args, 2)
	if err != nil {
		return nil, err
	}
	var result launchpadkeeper.SwapResult
	if method.Name == QuoteBuyMethod {
		result, err = p.keeper.QuoteBuy(ctx, market.Denom, args[1].(*big.Int))
	} else {
		result, err = p.keeper.QuoteSell(ctx, market.Denom, args[1].(*big.Int))
	}
	if err != nil {
		return nil, err
	}
	return method.Outputs.Pack(result.AmountOut, result.FeeBps, result.FeeAmount)
}
