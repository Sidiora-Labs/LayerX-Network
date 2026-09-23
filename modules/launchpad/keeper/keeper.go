package keeper

import (
	"encoding/binary"
	"encoding/json"
	"fmt"

	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/modules/launchpad/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	authtypes "github.com/sidiora-labs/paxeer-network/sdk/x/auth/types"
	govtypes "github.com/sidiora-labs/paxeer-network/sdk/x/gov/types"
)

// Keeper owns the native launchpad: every market's token supply, real quote
// balance and undistributed fees sit in the launchpad module account, and
// only swaps and fee-strategy executions move them.
type Keeper struct {
	storeKey sdk.StoreKey

	bankKeeper   types.BankKeeper
	tokenFactory types.TokenFactory
	evmKeeper    types.EVMKeeper

	authority string
}

// NewKeeper wires the launchpad keeper. Params change only through
// UpdateParams signed by the governance module account.
func NewKeeper(storeKey sdk.StoreKey, bankKeeper types.BankKeeper, tokenFactory types.TokenFactory,
	evmKeeper types.EVMKeeper) *Keeper {
	return &Keeper{storeKey: storeKey, bankKeeper: bankKeeper, tokenFactory: tokenFactory, evmKeeper: evmKeeper,
		authority: authtypes.NewModuleAddress(govtypes.ModuleName).String()}
}

func (k *Keeper) store(ctx sdk.Context) sdk.KVStore { return ctx.KVStore(k.storeKey) }

// ModuleAddress is the escrow account of every market.
func (k *Keeper) ModuleAddress() sdk.AccAddress { return authtypes.NewModuleAddress(types.ModuleName) }

// TreasuryAddress receives creation fees and the protocol share of buy fees.
func (k *Keeper) TreasuryAddress() sdk.AccAddress {
	return authtypes.NewModuleAddress(types.TreasuryName)
}

// Authority is the bech32 account allowed to change params.
func (k *Keeper) Authority() string { return k.authority }

func (k *Keeper) GetParams(ctx sdk.Context) types.Params {
	bz := k.store(ctx).Get(types.ParamsKey)
	if bz == nil {
		return types.DefaultParams()
	}
	var params types.Params
	if err := json.Unmarshal(bz, &params); err != nil {
		panic(fmt.Errorf("launchpad: corrupt params: %w", err))
	}
	return params
}

func (k *Keeper) setParams(ctx sdk.Context, params types.Params) error {
	if err := params.Validate(); err != nil {
		return err
	}
	bz, err := json.Marshal(params)
	if err != nil {
		return err
	}
	k.store(ctx).Set(types.ParamsKey, bz)
	return nil
}

// UpdateParams replaces the params when signed by the governance module
// account, which is how ProtocolConfig's admin setters are reached natively.
func (k *Keeper) UpdateParams(ctx sdk.Context, authority string, params types.Params) error {
	if authority != k.authority {
		return fmt.Errorf("%w: %s", types.ErrUnauthorized, authority)
	}
	if err := k.setParams(ctx, params); err != nil {
		return err
	}
	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventTypeParamsUpdated))
	return nil
}

func (k *Keeper) getUint64(ctx sdk.Context, key []byte) uint64 {
	bz := k.store(ctx).Get(key)
	if bz == nil {
		return 0
	}
	return binary.BigEndian.Uint64(bz)
}

func (k *Keeper) setUint64(ctx sdk.Context, key []byte, value uint64) {
	k.store(ctx).Set(key, binary.BigEndian.AppendUint64(nil, value))
}

// GetMarketCount is the number of markets ever created.
func (k *Keeper) GetMarketCount(ctx sdk.Context) uint64 {
	return k.getUint64(ctx, types.MarketCountKey)
}

// GetProtocolFeesPending is FeeAccumulator's running total of protocol cuts.
func (k *Keeper) GetProtocolFeesPending(ctx sdk.Context) sdk.Int {
	bz := k.store(ctx).Get(types.ProtocolFeesPendingKey)
	if bz == nil {
		return sdk.ZeroInt()
	}
	var out sdk.Int
	if err := out.Unmarshal(bz); err != nil {
		panic(fmt.Errorf("launchpad: corrupt protocol fees: %w", err))
	}
	return out
}

func (k *Keeper) setProtocolFeesPending(ctx sdk.Context, value sdk.Int) {
	bz, err := value.Marshal()
	if err != nil {
		panic(err)
	}
	k.store(ctx).Set(types.ProtocolFeesPendingKey, bz)
}

func (k *Keeper) GetMarket(ctx sdk.Context, denom string) (types.Market, bool) {
	bz := k.store(ctx).Get(types.MarketKey(denom))
	if bz == nil {
		return types.Market{}, false
	}
	var market types.Market
	if err := json.Unmarshal(bz, &market); err != nil {
		panic(fmt.Errorf("launchpad: corrupt market %s: %w", denom, err))
	}
	return market, true
}

func (k *Keeper) mustMarket(ctx sdk.Context, denom string) (types.Market, error) {
	market, found := k.GetMarket(ctx, denom)
	if !found {
		return types.Market{}, fmt.Errorf("%w: %s", types.ErrUnknownMarket, denom)
	}
	return market, nil
}

func (k *Keeper) setMarket(ctx sdk.Context, market types.Market) {
	bz, err := json.Marshal(market)
	if err != nil {
		panic(err)
	}
	k.store(ctx).Set(types.MarketKey(market.Denom), bz)
}

// indexMarket writes a market's position, pointer and creator indexes.
func (k *Keeper) indexMarket(ctx sdk.Context, market types.Market) {
	store := k.store(ctx)
	store.Set(types.MarketIndexKey(market.Index), []byte(market.Denom))
	if common.IsHexAddress(market.Pointer) {
		store.Set(types.MarketPointerKey(common.HexToAddress(market.Pointer)), []byte(market.Denom))
	}
	creator := sdk.MustAccAddressFromBech32(market.Creator)
	store.Set(types.CreatorMarketKey(creator, market.Index), []byte(market.Denom))
}

// GetMarketByIndex resolves the index-th market (1-based, creation order).
func (k *Keeper) GetMarketByIndex(ctx sdk.Context, index uint64) (types.Market, bool) {
	denom := k.store(ctx).Get(types.MarketIndexKey(index))
	if denom == nil {
		return types.Market{}, false
	}
	return k.GetMarket(ctx, string(denom))
}

// GetMarketByPointer resolves a market from its ERC20 pointer.
func (k *Keeper) GetMarketByPointer(ctx sdk.Context, pointer common.Address) (types.Market, bool) {
	denom := k.store(ctx).Get(types.MarketPointerKey(pointer))
	if denom == nil {
		return types.Market{}, false
	}
	return k.GetMarket(ctx, string(denom))
}

// GetMarkets returns up to limit markets starting at the offset-th (0-based)
// in creation order.
func (k *Keeper) GetMarkets(ctx sdk.Context, offset, limit uint64) []types.Market {
	count := k.GetMarketCount(ctx)
	out := []types.Market{}
	for index := offset + 1; index <= count && uint64(len(out)) < limit && index > offset; index++ {
		if market, found := k.GetMarketByIndex(ctx, index); found {
			out = append(out, market)
		}
	}
	return out
}

// GetMarketsByCreator returns up to limit of creator's markets after skipping
// offset of them, in creation order.
func (k *Keeper) GetMarketsByCreator(ctx sdk.Context, creator sdk.AccAddress, offset, limit uint64) []types.Market {
	out := []types.Market{}
	iterator := sdk.KVStorePrefixIterator(k.store(ctx), types.CreatorMarketPrefixFor(creator))
	defer iterator.Close()
	skipped := uint64(0)
	for ; iterator.Valid() && uint64(len(out)) < limit; iterator.Next() {
		if skipped < offset {
			skipped++
			continue
		}
		if market, found := k.GetMarket(ctx, string(iterator.Value())); found {
			out = append(out, market)
		}
	}
	return out
}

// IterateMarkets visits every market in creation order until cb returns true.
func (k *Keeper) IterateMarkets(ctx sdk.Context, cb func(types.Market) bool) {
	count := k.GetMarketCount(ctx)
	for index := uint64(1); index <= count; index++ {
		market, found := k.GetMarketByIndex(ctx, index)
		if found && cb(market) {
			return
		}
	}
}

func (k *Keeper) GetAirdropEpochAmount(ctx sdk.Context, denom string, epoch uint64) sdk.Int {
	bz := k.store(ctx).Get(types.AirdropEpochKey(denom, epoch))
	if bz == nil {
		return sdk.ZeroInt()
	}
	var out sdk.Int
	if err := out.Unmarshal(bz); err != nil {
		panic(fmt.Errorf("launchpad: corrupt airdrop epoch: %w", err))
	}
	return out
}

func (k *Keeper) setAirdropEpochAmount(ctx sdk.Context, denom string, epoch uint64, amount sdk.Int) {
	bz, err := amount.Marshal()
	if err != nil {
		panic(err)
	}
	k.store(ctx).Set(types.AirdropEpochKey(denom, epoch), bz)
}

// HasClaimedAirdrop reports whether holder claimed denom's airdrop at epoch.
func (k *Keeper) HasClaimedAirdrop(ctx sdk.Context, denom string, holder sdk.AccAddress, epoch uint64) bool {
	return k.store(ctx).Has(types.AirdropClaimKey(denom, holder, epoch))
}

func (k *Keeper) setAirdropClaimed(ctx sdk.Context, denom string, holder sdk.AccAddress, epoch uint64) {
	k.store(ctx).Set(types.AirdropClaimKey(denom, holder, epoch), []byte{1})
}
