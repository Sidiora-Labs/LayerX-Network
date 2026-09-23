package keeper

import (
	"math/big"
	"strconv"

	"github.com/sidiora-labs/paxeer-network/modules/launchpad/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// SwapResult is what a buy or sell moves.
type SwapResult struct {
	AmountIn    *big.Int
	AmountOut   *big.Int
	FeeBps      *big.Int
	FeeAmount   *big.Int
	ProtocolCut *big.Int
	PoolCut     *big.Int
}

// quote runs SidioraPool.swap's arithmetic on market without moving funds
// and returns the market as the swap leaves it.
func quote(params types.Params, market types.Market, now int64, isBuy bool, amountIn *big.Int) (SwapResult, types.Market, error) {
	if amountIn == nil || amountIn.Sign() == 0 {
		return SwapResult{}, market, types.ErrInsufficientInput
	}
	fee, err := feeBps(params, market, now)
	if err != nil {
		return SwapResult{}, market, err
	}
	feeAmount, err := types.FeeAmount(amountIn, fee)
	if err != nil {
		return SwapResult{}, market, err
	}
	afterFee := new(big.Int).Sub(amountIn, feeAmount)
	effective := market.EffectiveQuote()
	tokenReserve := market.TokenReserve.BigInt()
	real := market.RealQuoteBalance.BigInt()
	result := SwapResult{AmountIn: new(big.Int).Set(amountIn), FeeBps: fee, FeeAmount: feeAmount,
		ProtocolCut: new(big.Int), PoolCut: new(big.Int)}

	if isBuy {
		out, err := types.GetAmountOut(effective, tokenReserve, afterFee)
		if err != nil {
			return SwapResult{}, market, err
		}
		if out.Cmp(tokenReserve) > 0 {
			return SwapResult{}, market, types.ErrInsufficientLiquidity
		}
		result.AmountOut = out
		market.RealQuoteBalance = sdk.NewIntFromBigInt(new(big.Int).Add(real, afterFee))
		market.TokenReserve = sdk.NewIntFromBigInt(new(big.Int).Sub(tokenReserve, out))
		if feeAmount.Sign() > 0 {
			// FeeAccumulator.recordFee: protocolFeeBps of the fee to the
			// treasury, the rest to the market's fee-rights holder.
			protocolCut, err := types.FeeAmount(feeAmount, new(big.Int).SetUint64(params.ProtocolFeeBps))
			if err != nil {
				return SwapResult{}, market, err
			}
			result.ProtocolCut = protocolCut
			result.PoolCut = new(big.Int).Sub(feeAmount, protocolCut)
			market.AccumulatedQuoteFees = market.AccumulatedQuoteFees.Add(sdk.NewIntFromBigInt(feeAmount))
			market.AccumulatedFees = market.AccumulatedFees.Add(sdk.NewIntFromBigInt(result.PoolCut))
		}
	} else {
		out, err := types.GetAmountOut(tokenReserve, effective, afterFee)
		if err != nil {
			return SwapResult{}, market, err
		}
		// The virtual floor: virtual quote prices the curve but is never paid.
		if out.Cmp(real) > 0 {
			return SwapResult{}, market, types.ErrVirtualFloorBreached
		}
		result.AmountOut = out
		reserve := new(big.Int).Add(tokenReserve, amountIn)
		if reserve.Cmp(types.MaxUint256()) > 0 {
			return SwapResult{}, market, types.ErrOverflow
		}
		market.TokenReserve = sdk.NewIntFromBigInt(reserve)
		market.RealQuoteBalance = sdk.NewIntFromBigInt(new(big.Int).Sub(real, out))
		if feeAmount.Sign() > 0 {
			market.AccumulatedTokenFees = market.AccumulatedTokenFees.Add(sdk.NewIntFromBigInt(feeAmount))
		}
	}

	volume := new(big.Int).Add(market.CumulativeVolume.BigInt(), amountIn)
	if volume.Cmp(types.MaxUint256()) > 0 {
		return SwapResult{}, market, types.ErrOverflow
	}
	market.CumulativeVolume = sdk.NewIntFromBigInt(volume)
	price, err := market.Price()
	if err != nil {
		return SwapResult{}, market, err
	}
	snapshots := append([]sdk.Int(nil), market.PriceSnapshots...)
	snapshots[market.SnapshotIndex] = sdk.NewIntFromBigInt(price)
	market.PriceSnapshots = snapshots
	market.SnapshotIndex = (market.SnapshotIndex + 1) % types.SnapshotSlots
	if market.SnapshotCount < types.SnapshotSlots {
		market.SnapshotCount++
	}
	return result, market, nil
}

// QuoteBuy is the outcome of buying with quoteIn now.
func (k *Keeper) QuoteBuy(ctx sdk.Context, denom string, quoteIn *big.Int) (SwapResult, error) {
	return k.quoteView(ctx, denom, true, quoteIn)
}

// QuoteSell is the outcome of selling amountIn tokens now.
func (k *Keeper) QuoteSell(ctx sdk.Context, denom string, amountIn *big.Int) (SwapResult, error) {
	return k.quoteView(ctx, denom, false, amountIn)
}

func (k *Keeper) quoteView(ctx sdk.Context, denom string, isBuy bool, amountIn *big.Int) (SwapResult, error) {
	market, err := k.mustMarket(ctx, denom)
	if err != nil {
		return SwapResult{}, err
	}
	result, _, err := quote(k.GetParams(ctx), market, ctx.BlockTime().Unix(), isBuy, amountIn)
	return result, err
}

// Swap is SidioraPool.swap. A buy takes amountIn of the quote denom from
// trader and pays tokens to recipient; a sell takes amountIn tokens and pays
// quote. Fees are charged in the input token: the buy fee's protocol share
// goes to the treasury and the rest accrues to the fee-rights holder, the
// sell fee stays in the token reserve.
func (k *Keeper) Swap(ctx sdk.Context, trader sdk.AccAddress, denom string, isBuy bool, amountIn, minAmountOut *big.Int,
	recipient sdk.AccAddress, deadline uint64) (SwapResult, error) {
	market, err := k.mustMarket(ctx, denom)
	if err != nil {
		return SwapResult{}, err
	}
	if market.Paused {
		return SwapResult{}, types.ErrPaused
	}
	now := ctx.BlockTime().Unix()
	if now < 0 || uint64(now) > deadline {
		return SwapResult{}, types.ErrDeadlineExpired
	}
	if amountIn == nil || amountIn.Sign() == 0 {
		return SwapResult{}, types.ErrInsufficientInput
	}
	if recipient.Empty() || trader.Empty() {
		return SwapResult{}, types.ErrZeroAddress
	}
	params := k.GetParams(ctx)
	result, updated, err := quote(params, market, now, isBuy, amountIn)
	if err != nil {
		return SwapResult{}, err
	}
	if minAmountOut != nil && result.AmountOut.Cmp(minAmountOut) < 0 {
		return SwapResult{}, types.ErrSlippageExceeded
	}

	escrow := k.ModuleAddress()
	inDenom, outDenom := denom, params.QuoteDenom
	if isBuy {
		inDenom, outDenom = params.QuoteDenom, denom
	}
	if err := k.bankKeeper.SendCoins(ctx, trader, escrow, coins(inDenom, amountIn)); err != nil {
		return SwapResult{}, err
	}
	if err := k.bankKeeper.SendCoins(ctx, escrow, recipient, coins(outDenom, result.AmountOut)); err != nil {
		return SwapResult{}, err
	}
	if result.ProtocolCut.Sign() > 0 {
		if err := k.bankKeeper.SendCoins(ctx, escrow, k.TreasuryAddress(), coins(params.QuoteDenom, result.ProtocolCut)); err != nil {
			return SwapResult{}, err
		}
		k.setProtocolFeesPending(ctx, k.GetProtocolFeesPending(ctx).Add(sdk.NewIntFromBigInt(result.ProtocolCut)))
	}
	k.setMarket(ctx, updated)

	price, err := updated.Price()
	if err != nil {
		return SwapResult{}, err
	}
	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventTypeSwap,
		sdk.NewAttribute(types.AttributeKeyDenom, denom),
		sdk.NewAttribute(types.AttributeKeyTrader, trader.String()),
		sdk.NewAttribute(types.AttributeKeyRecipient, recipient.String()),
		sdk.NewAttribute(types.AttributeKeyIsBuy, strconv.FormatBool(isBuy)),
		sdk.NewAttribute(types.AttributeKeyAmountIn, amountIn.String()),
		sdk.NewAttribute(types.AttributeKeyAmountOut, result.AmountOut.String()),
		sdk.NewAttribute(types.AttributeKeyFee, result.FeeAmount.String()),
		sdk.NewAttribute(types.AttributeKeyFeeBps, result.FeeBps.String()),
		sdk.NewAttribute(types.AttributeKeyPrice, price.String())))
	if isBuy && result.FeeAmount.Sign() > 0 {
		ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventTypeFeeRecorded,
			sdk.NewAttribute(types.AttributeKeyDenom, denom),
			sdk.NewAttribute(types.AttributeKeyFee, result.FeeAmount.String()),
			sdk.NewAttribute(types.AttributeKeyProtocolCut, result.ProtocolCut.String()),
			sdk.NewAttribute(types.AttributeKeyPoolCut, result.PoolCut.String())))
	}
	return result, nil
}

func coins(denom string, amount *big.Int) sdk.Coins {
	if amount.Sign() == 0 {
		return sdk.NewCoins()
	}
	return sdk.NewCoins(sdk.NewCoin(denom, sdk.NewIntFromBigInt(amount)))
}
