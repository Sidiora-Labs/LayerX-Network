package antedecorators

import (
	"fmt"
	"math/big"

	evmkeeper "github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
	"github.com/sidiora-labs/paxeer-network/sdk/x/auth/ante"
	paramskeeper "github.com/sidiora-labs/paxeer-network/sdk/x/params/keeper"
)

type convertedFeeTx struct {
	sdk.FeeTx
	fee sdk.Coins
}

func (tx convertedFeeTx) GetFee() sdk.Coins { return tx.fee }

func NewFeeDenomTxFeeChecker(k *evmkeeper.Keeper, inner ante.TxFeeChecker) ante.TxFeeChecker {
	if inner == nil {
		inner = ante.CheckTxFeeWithValidatorMinGasPrices
	}
	return func(ctx sdk.Context, tx sdk.Tx, simulate bool, paramsKeeper paramskeeper.Keeper) (sdk.Coins, int64, error) {
		if !k.GetFeeTokenEnabled(ctx) {
			return inner(ctx, tx, simulate, paramsKeeper)
		}
		feeTx, ok := tx.(sdk.FeeTx)
		if !ok {
			return nil, 0, sdkerrors.Wrap(sdkerrors.ErrTxDecode, "Tx must be a FeeTx")
		}
		offered := feeTx.GetFee()
		if !offered.IsValid() {
			return nil, 0, sdkerrors.Wrapf(sdkerrors.ErrInsufficientFee, "invalid fee amount: %s", offered)
		}
		baseDenom := k.GetBaseDenom(ctx)
		feeParams := paramsKeeper.GetFeesParams(ctx)
		authAllowed := feeParams.GetAllowedFeeDenoms()
		converted := sdk.Coins{}
		baseAmount := new(big.Int)
		hasFeeToken := false
		for _, coin := range offered {
			if coin.Denom == baseDenom {
				baseAmount.Add(baseAmount, coin.Amount.BigInt())
				continue
			}
			if allowed, _ := k.IsAllowedFeeDenom(ctx, coin.Denom); allowed {
				rate, err := k.GetFeeTokenRate(ctx, coin.Denom)
				if err != nil {
					return nil, 0, fmt.Errorf("fee denom %q: %w", coin.Denom, err)
				}
				amount, err := k.ConvertFeeFromDenom(coin.Amount, rate, false)
				if err != nil {
					return nil, 0, fmt.Errorf("fee denom %q: %w", coin.Denom, err)
				}
				baseAmount.Add(baseAmount, amount.BigInt())
				hasFeeToken = true
				continue
			}
			allowed := false
			for _, denom := range authAllowed {
				if denom == coin.Denom {
					allowed = true
					break
				}
			}
			if !allowed {
				return nil, 0, fmt.Errorf("%w: %q: %w", evmkeeper.ErrFeeTokenDenomNotAllowed, coin.Denom, evmkeeper.ErrFeeTokenRateUnavailable)
			}
			converted = append(converted, coin)
		}
		if !hasFeeToken {
			return inner(ctx, tx, simulate, paramsKeeper)
		}
		// Mixed fees add network coins to each allowed token's rounded-down Paxeer value, retaining auth-approved coins at their existing value.
		if baseAmount.BitLen() > 256 {
			return nil, 0, fmt.Errorf("fee denoms %s: %w", offered.String(), evmkeeper.ErrFeeTokenOverflow)
		}
		converted = converted.Add(sdk.NewCoin(baseDenom, sdk.NewIntFromBigInt(baseAmount)))
		_, priority, err := inner(ctx, convertedFeeTx{FeeTx: feeTx, fee: converted}, simulate, paramsKeeper)
		if err != nil {
			return nil, 0, err
		}
		return offered, priority, nil
	}
}
