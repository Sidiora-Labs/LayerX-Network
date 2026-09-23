package keeper

import (
	"strconv"

	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/modules/launchpad/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

func (k *Keeper) requireFeeRightsHolder(ctx sdk.Context, caller sdk.AccAddress, denom string) (types.Market, error) {
	market, err := k.mustMarket(ctx, denom)
	if err != nil {
		return types.Market{}, err
	}
	if market.FeeRightsHolder != caller.String() {
		return types.Market{}, types.ErrNotFeeRightsHolder
	}
	return market, nil
}

func (k *Keeper) requireStrategy(ctx sdk.Context, caller sdk.AccAddress, denom string, strategy types.FeeStrategy) (types.Market, error) {
	market, err := k.requireFeeRightsHolder(ctx, caller, denom)
	if err != nil {
		return types.Market{}, err
	}
	if market.FeeStrategy != strategy {
		return types.Market{}, types.ErrWrongStrategy
	}
	return market, nil
}

// takeAccumulatedFees zeroes the market's pool-cut fees and returns them.
func (k *Keeper) takeAccumulatedFees(market *types.Market) (sdk.Int, error) {
	amount := market.AccumulatedFees
	if !amount.IsPositive() {
		return sdk.Int{}, types.ErrNoFeesAccumulated
	}
	market.AccumulatedFees = sdk.ZeroInt()
	return amount, nil
}

func (k *Keeper) payQuote(ctx sdk.Context, to sdk.AccAddress, amount sdk.Int) error {
	denom := k.GetParams(ctx).QuoteDenom
	return k.bankKeeper.SendCoins(ctx, k.ModuleAddress(), to, sdk.NewCoins(sdk.NewCoin(denom, amount)))
}

// SetFeeStrategy is FeesRouter.setFeeStrategy.
func (k *Keeper) SetFeeStrategy(ctx sdk.Context, caller sdk.AccAddress, denom string, strategy types.FeeStrategy) error {
	market, err := k.requireFeeRightsHolder(ctx, caller, denom)
	if err != nil {
		return err
	}
	if err := strategy.Validate(); err != nil {
		return err
	}
	old := market.FeeStrategy
	market.FeeStrategy = strategy
	k.setMarket(ctx, market)
	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventTypeFeeStrategyChanged,
		sdk.NewAttribute(types.AttributeKeyDenom, denom),
		sdk.NewAttribute("old_strategy", strconv.FormatUint(uint64(old), 10)),
		sdk.NewAttribute(types.AttributeKeyStrategy, strconv.FormatUint(uint64(strategy), 10))))
	return nil
}

// ClaimFees is FeesRouter.claimFees: the CLAIM strategy pays the market's
// accumulated fees to recipient.
func (k *Keeper) ClaimFees(ctx sdk.Context, caller sdk.AccAddress, denom string, recipient sdk.AccAddress) (sdk.Int, error) {
	market, err := k.requireStrategy(ctx, caller, denom, types.FeeStrategyClaim)
	if err != nil {
		return sdk.Int{}, err
	}
	if recipient.Empty() {
		return sdk.Int{}, types.ErrZeroAddress
	}
	amount, err := k.takeAccumulatedFees(&market)
	if err != nil {
		return sdk.Int{}, err
	}
	k.setMarket(ctx, market)
	if err := k.payQuote(ctx, recipient, amount); err != nil {
		return sdk.Int{}, err
	}
	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventTypeFeesClaimed,
		sdk.NewAttribute(types.AttributeKeyDenom, denom),
		sdk.NewAttribute(types.AttributeKeyRecipient, recipient.String()),
		sdk.NewAttribute(types.AttributeKeyAmount, amount.String())))
	return amount, nil
}

// DeadAccount is the bank account of 0x…dEaD, where burned fees go.
func (k *Keeper) DeadAccount(ctx sdk.Context) sdk.AccAddress {
	return k.evmKeeper.GetPaxAddressOrDefault(ctx, common.HexToAddress(types.DeadAddress))
}

// ExecuteBurn is FeesRouter.executeBurn: the BURN strategy sends the
// accumulated fees to 0x…dEaD.
func (k *Keeper) ExecuteBurn(ctx sdk.Context, caller sdk.AccAddress, denom string) (sdk.Int, error) {
	market, err := k.requireStrategy(ctx, caller, denom, types.FeeStrategyBurn)
	if err != nil {
		return sdk.Int{}, err
	}
	amount, err := k.takeAccumulatedFees(&market)
	if err != nil {
		return sdk.Int{}, err
	}
	k.setMarket(ctx, market)
	if err := k.payQuote(ctx, k.DeadAccount(ctx), amount); err != nil {
		return sdk.Int{}, err
	}
	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventTypeFeesBurned,
		sdk.NewAttribute(types.AttributeKeyDenom, denom),
		sdk.NewAttribute(types.AttributeKeyAmount, amount.String())))
	return amount, nil
}

// ExecuteAirdrop is FeesRouter.executeAirdrop: the AIRDROP strategy opens a
// new epoch holding the accumulated fees for token holders to claim.
func (k *Keeper) ExecuteAirdrop(ctx sdk.Context, caller sdk.AccAddress, denom string) (sdk.Int, error) {
	market, err := k.requireStrategy(ctx, caller, denom, types.FeeStrategyAirdrop)
	if err != nil {
		return sdk.Int{}, err
	}
	amount, err := k.takeAccumulatedFees(&market)
	if err != nil {
		return sdk.Int{}, err
	}
	market.AirdropEpoch++
	market.AirdropBalance = market.AirdropBalance.Add(amount)
	k.setMarket(ctx, market)
	k.setAirdropEpochAmount(ctx, denom, market.AirdropEpoch, amount)
	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventTypeAirdropTriggered,
		sdk.NewAttribute(types.AttributeKeyDenom, denom),
		sdk.NewAttribute(types.AttributeKeyAmount, amount.String()),
		sdk.NewAttribute(types.AttributeKeyEpoch, strconv.FormatUint(market.AirdropEpoch, 10))))
	return amount, nil
}

// ClaimAirdrop is FeeAccumulator.claimAirdrop: any holder takes
// epochAmount*balance/totalSupply of the current epoch once.
func (k *Keeper) ClaimAirdrop(ctx sdk.Context, holder sdk.AccAddress, denom string) (sdk.Int, error) {
	market, err := k.mustMarket(ctx, denom)
	if err != nil {
		return sdk.Int{}, err
	}
	epoch := market.AirdropEpoch
	if epoch == 0 {
		return sdk.Int{}, types.ErrAirdropNotTriggered
	}
	if k.HasClaimedAirdrop(ctx, denom, holder, epoch) {
		return sdk.Int{}, types.ErrAlreadyClaimed
	}
	epochAmount := k.GetAirdropEpochAmount(ctx, denom, epoch)
	if !epochAmount.IsPositive() {
		return sdk.Int{}, types.ErrNoFeesAccumulated
	}
	balance := k.bankKeeper.GetBalance(ctx, holder, denom).Amount
	supply := k.bankKeeper.GetSupply(ctx, denom).Amount
	if !balance.IsPositive() || !supply.IsPositive() {
		return sdk.Int{}, types.ErrZeroAmount
	}
	product, err := types.MulDiv(epochAmount.BigInt(), balance.BigInt(), supply.BigInt())
	if err != nil {
		return sdk.Int{}, err
	}
	amount := sdk.NewIntFromBigInt(product)
	if !amount.IsPositive() {
		return sdk.Int{}, types.ErrZeroAmount
	}
	if amount.GT(market.AirdropBalance) {
		return sdk.Int{}, types.ErrOverflow
	}
	k.setAirdropClaimed(ctx, denom, holder, epoch)
	market.AirdropBalance = market.AirdropBalance.Sub(amount)
	k.setMarket(ctx, market)
	if err := k.payQuote(ctx, holder, amount); err != nil {
		return sdk.Int{}, err
	}
	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventTypeAirdropClaimed,
		sdk.NewAttribute(types.AttributeKeyDenom, denom),
		sdk.NewAttribute(types.AttributeKeyHolder, holder.String()),
		sdk.NewAttribute(types.AttributeKeyAmount, amount.String()),
		sdk.NewAttribute(types.AttributeKeyEpoch, strconv.FormatUint(epoch, 10))))
	return amount, nil
}

// ExecuteLpRewards is FeesRouter.executeLpRewards: the LP_REWARDS strategy
// returns the accumulated fees to the curve's real quote balance, which is
// what the pool's syncReserves picks up.
func (k *Keeper) ExecuteLpRewards(ctx sdk.Context, caller sdk.AccAddress, denom string) (sdk.Int, error) {
	market, err := k.requireStrategy(ctx, caller, denom, types.FeeStrategyLpRewards)
	if err != nil {
		return sdk.Int{}, err
	}
	amount, err := k.takeAccumulatedFees(&market)
	if err != nil {
		return sdk.Int{}, err
	}
	market.RealQuoteBalance = market.RealQuoteBalance.Add(amount)
	k.setMarket(ctx, market)
	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventTypeLpRewardsSent,
		sdk.NewAttribute(types.AttributeKeyDenom, denom),
		sdk.NewAttribute(types.AttributeKeyAmount, amount.String())))
	return amount, nil
}

// Pause is SidioraPool.pause: the guardian halts swaps.
func (k *Keeper) Pause(ctx sdk.Context, caller sdk.AccAddress, denom string) error {
	return k.setPaused(ctx, caller, denom, true)
}

// Unpause is SidioraPool.unpause.
func (k *Keeper) Unpause(ctx sdk.Context, caller sdk.AccAddress, denom string) error {
	return k.setPaused(ctx, caller, denom, false)
}

func (k *Keeper) setPaused(ctx sdk.Context, caller sdk.AccAddress, denom string, paused bool) error {
	market, err := k.mustMarket(ctx, denom)
	if err != nil {
		return err
	}
	if market.Guardian != caller.String() {
		return types.ErrNotGuardian
	}
	if market.Paused == paused {
		if paused {
			return types.ErrPaused
		}
		return types.ErrNotPaused
	}
	market.Paused = paused
	k.setMarket(ctx, market)
	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventTypePauseToggled,
		sdk.NewAttribute(types.AttributeKeyDenom, denom),
		sdk.NewAttribute(types.AttributeKeyPaused, strconv.FormatBool(paused))))
	return nil
}
