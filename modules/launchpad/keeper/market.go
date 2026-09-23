package keeper

import (
	"fmt"
	"math/big"
	"strconv"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/vm"
	"github.com/sidiora-labs/paxeer-network/modules/launchpad/types"
	tokenfactorytypes "github.com/sidiora-labs/paxeer-network/modules/tokenfactory/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	banktypes "github.com/sidiora-labs/paxeer-network/sdk/x/bank/types"
	"github.com/sidiora-labs/paxeer-network/utils"
)

// Subdenom is the tokenfactory subdenom of the index-th market.
func Subdenom(index uint64) string { return "lp" + strconv.FormatUint(index, 10) }

// CreateMarket is SidioraFactory.createMarket: it charges the creation fee
// in the quote denom to the treasury, creates a fixed-supply tokenfactory
// denom administered by the launchpad account, mints the whole supply into
// the launchpad escrow, registers the denom's ERC20 pointer and opens the
// virtual-reserve curve with the creator as guardian and fee-rights holder.
// evm is the calling EVM when entered through the precompile; without one a
// one-off EVM deploys the pointer.
func (k *Keeper) CreateMarket(ctx sdk.Context, evm *vm.EVM, creator sdk.AccAddress, name, symbol string,
	strategy types.FeeStrategy) (types.Market, error) {
	if creator.Empty() {
		return types.Market{}, types.ErrZeroAddress
	}
	if err := strategy.Validate(); err != nil {
		return types.Market{}, err
	}
	if err := types.ValidateNameSymbol(name, symbol); err != nil {
		return types.Market{}, err
	}
	params := k.GetParams(ctx)
	if params.CreationFee.IsPositive() {
		fee := sdk.NewCoins(sdk.NewCoin(params.QuoteDenom, params.CreationFee))
		if err := k.bankKeeper.SendCoins(ctx, creator, k.TreasuryAddress(), fee); err != nil {
			return types.Market{}, err
		}
	}

	index := k.GetMarketCount(ctx) + 1
	k.setUint64(ctx, types.MarketCountKey, index)
	admin := k.ModuleAddress().String()
	goCtx := sdk.WrapSDKContext(ctx)
	created, err := k.tokenFactory.CreateDenom(goCtx, &tokenfactorytypes.MsgCreateDenom{Sender: admin, Subdenom: Subdenom(index)})
	if err != nil {
		return types.Market{}, err
	}
	denom := created.NewTokenDenom
	display := denom + "/display"
	metadata := banktypes.Metadata{
		Description: name,
		DenomUnits: []*banktypes.DenomUnit{
			{Denom: denom, Exponent: 0},
			{Denom: display, Exponent: types.TokenDecimals, Aliases: []string{symbol}},
		},
		Base: denom, Display: display, Name: name, Symbol: symbol,
	}
	if _, err := k.tokenFactory.SetDenomMetadata(goCtx, &tokenfactorytypes.MsgSetDenomMetadata{Sender: admin, Metadata: metadata}); err != nil {
		return types.Market{}, err
	}
	supply := params.VirtualTokenDefault
	if _, err := k.tokenFactory.Mint(goCtx, &tokenfactorytypes.MsgMint{Sender: admin, Amount: sdk.NewCoin(denom, supply)}); err != nil {
		return types.Market{}, err
	}
	pointer, err := k.registerPointer(ctx, evm, denom, utils.ERCMetadata{Name: name, Symbol: symbol, Decimals: types.TokenDecimals})
	if err != nil {
		return types.Market{}, err
	}

	snapshots := make([]sdk.Int, types.SnapshotSlots)
	for i := range snapshots {
		snapshots[i] = sdk.ZeroInt()
	}
	market := types.Market{
		Denom: denom, Index: index, Name: name, Symbol: symbol, Pointer: pointer.Hex(),
		Creator: creator.String(), Guardian: creator.String(), FeeRightsHolder: creator.String(), FeeStrategy: strategy,
		TotalSupply: supply, VirtualQuoteReserve: params.VirtualQuoteDefault, RealQuoteBalance: sdk.ZeroInt(),
		TokenReserve: supply, CreationTime: ctx.BlockTime().Unix(), CumulativeVolume: sdk.ZeroInt(),
		PriceSnapshots: snapshots, AccumulatedQuoteFees: sdk.ZeroInt(), AccumulatedTokenFees: sdk.ZeroInt(),
		AccumulatedFees: sdk.ZeroInt(), AirdropBalance: sdk.ZeroInt(),
	}
	k.setMarket(ctx, market)
	k.indexMarket(ctx, market)
	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventTypeMarketCreated,
		sdk.NewAttribute(types.AttributeKeyDenom, denom),
		sdk.NewAttribute(types.AttributeKeyPointer, market.Pointer),
		sdk.NewAttribute(types.AttributeKeyCreator, market.Creator),
		sdk.NewAttribute(types.AttributeKeyStrategy, strconv.FormatUint(uint64(strategy), 10))))
	return market, nil
}

func (k *Keeper) registerPointer(ctx sdk.Context, evm *vm.EVM, denom string, metadata utils.ERCMetadata) (common.Address, error) {
	if evm != nil {
		return k.evmKeeper.UpsertERCNativePointer(ctx, evm, denom, metadata)
	}
	var pointer common.Address
	err := k.evmKeeper.RunWithOneOffEVMInstance(ctx, func(instance *vm.EVM) error {
		var upsertErr error
		pointer, upsertErr = k.evmKeeper.UpsertERCNativePointer(ctx, instance, denom, metadata)
		return upsertErr
	}, func(string, string) {})
	if err != nil {
		return common.Address{}, fmt.Errorf("launchpad: register pointer for %s: %w", denom, err)
	}
	return pointer, nil
}

// FeeBps is SidioraPool._calculateFee at the current block time.
func (k *Keeper) FeeBps(ctx sdk.Context, market types.Market) (*big.Int, error) {
	return feeBps(k.GetParams(ctx), market, ctx.BlockTime().Unix())
}

func feeBps(params types.Params, market types.Market, now int64) (*big.Int, error) {
	age := new(big.Int)
	if now > market.CreationTime {
		age.SetInt64(now - market.CreationTime)
	}
	volatility, err := types.CalculateVolatility(market.Snapshots(), market.SnapshotCount)
	if err != nil {
		return nil, err
	}
	return types.CalculateDynamicFee(params.FeeInputs(age, volatility))
}
