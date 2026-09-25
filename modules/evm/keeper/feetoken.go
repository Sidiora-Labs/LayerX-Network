package keeper

import (
	"encoding/json"
	"errors"
	"fmt"
	"github.com/sidiora-labs/paxeer-network/modules/evm/state"
	"github.com/sidiora-labs/paxeer-network/sdk/store/prefix"
	"math/big"

	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

func (k *Keeper) GetAccountFeeDenom(ctx sdk.Context, account common.Address) string {
	ctx = ctx.WithGasMeter(sdk.NewInfiniteGasMeterWithMultiplier(ctx))
	denom := ctx.KVStore(k.storeKey).Get(types.AccountFeeDenomKey(account))
	if denom == nil {
		return k.GetBaseDenom(ctx)
	}
	return string(denom)
}

func (k *Keeper) SetAccountFeeDenom(ctx sdk.Context, account common.Address, denom string) error {
	if !k.GetFeeTokenEnabled(ctx) {
		return fmt.Errorf("fee token preference: fee-token switch is off")
	}
	if allowed, _ := k.IsAllowedFeeDenom(ctx, denom); !allowed {
		return fmt.Errorf("fee token preference: denom %q is not allowed", denom)
	}
	ctx.KVStore(k.storeKey).Set(types.AccountFeeDenomKey(account), []byte(denom))
	return nil
}

func (k *Keeper) ClearAccountFeeDenom(ctx sdk.Context, account common.Address) {
	ctx.KVStore(k.storeKey).Delete(types.AccountFeeDenomKey(account))
}

var (
	ErrFeeTokenAmountInvalid   = errors.New("fee-token amount is invalid")
	ErrFeeTokenOverflow        = errors.New("fee-token conversion exceeds 256 bits")
	ErrFeeTokenDenomNotAllowed = errors.New("fee-token denom is not allowed")
)

// ConvertFeeToDenom converts Paxeer wei to base units; roundUp charges the network's ceiling and refunds the payer's ceiling.
func ConvertFeeToDenom(amount sdk.Int, rate sdk.Dec, roundUp bool) (sdk.Int, error) {
	return convertFeeAmount(amount, rate, roundUp, false)
}

// ConvertFeeFromDenom converts fee-denom base units back to Paxeer wei; roundUp selects ceiling instead of floor.
func ConvertFeeFromDenom(amount sdk.Int, rate sdk.Dec, roundUp bool) (sdk.Int, error) {
	return convertFeeAmount(amount, rate, roundUp, true)
}

func convertFeeAmount(amount sdk.Int, rate sdk.Dec, roundUp, inverse bool) (sdk.Int, error) {
	if rate.IsNil() || !rate.IsPositive() {
		return sdk.Int{}, ErrFeeTokenRateInvalid
	}
	if amount.IsNil() || amount.IsNegative() {
		return sdk.Int{}, ErrFeeTokenAmountInvalid
	}
	// Wide integer intermediates preserve all sdk.Dec digits before the checked sdk.Int result.
	scale := sdk.NewInt(1_000_000_000_000_000_000)
	numerator := amount.BigInt()
	denominator := new(big.Int).Mul(scale.BigInt(), scale.BigInt())
	if inverse {
		numerator.Mul(numerator, denominator)
		denominator = rate.BigInt()
	} else {
		numerator.Mul(numerator, rate.BigInt())
	}
	quotient, remainder := new(big.Int), new(big.Int)
	quotient.QuoRem(numerator, denominator, remainder)
	if roundUp && remainder.Sign() != 0 {
		quotient.Add(quotient, big.NewInt(1))
	}
	if quotient.BitLen() > 256 {
		return sdk.Int{}, ErrFeeTokenOverflow
	}
	return sdk.NewIntFromBigInt(quotient), nil
}

func (k *Keeper) ConvertFeeToDenom(amount sdk.Int, rate sdk.Dec, roundUp bool) (sdk.Int, error) {
	return ConvertFeeToDenom(amount, rate, roundUp)
}

func (k *Keeper) ConvertFeeFromDenom(amount sdk.Int, rate sdk.Dec, roundUp bool) (sdk.Int, error) {
	return ConvertFeeFromDenom(amount, rate, roundUp)
}

func (k *Keeper) GetFeeTokenCharge(ctx sdk.Context, payer common.Address) (*state.FeeTokenCharge, error) {
	if !k.GetFeeTokenEnabled(ctx) {
		return nil, nil
	}
	denom := k.GetAccountFeeDenom(ctx, payer)
	if denom == k.GetBaseDenom(ctx) {
		return nil, nil
	}
	if allowed, _ := k.IsAllowedFeeDenom(ctx, denom); !allowed {
		return nil, fmt.Errorf("%w: %q: %w", ErrFeeTokenDenomNotAllowed, denom, ErrFeeTokenRateUnavailable)
	}
	rate, err := k.GetFeeTokenRate(ctx, denom)
	if err != nil {
		return nil, err
	}
	return &state.FeeTokenCharge{Payer: payer, Denom: denom, Rate: rate}, nil
}

func (k *Keeper) SetAnteFeeTokenCharge(ctx sdk.Context, hash common.Hash, charge *state.FeeTokenCharge) error {
	store := prefix.NewStore(ctx.TransientStore(k.transientStoreKey), types.AnteFeeTokenChargePrefix)
	if charge == nil {
		store.Delete(hash[:])
		return nil
	}
	bz, err := json.Marshal(charge)
	if err != nil {
		return err
	}
	store.Set(hash[:], bz)
	return nil
}

func (k *Keeper) GetAnteFeeTokenCharge(ctx sdk.Context, hash common.Hash) (*state.FeeTokenCharge, error) {
	bz := prefix.NewStore(ctx.TransientStore(k.transientStoreKey), types.AnteFeeTokenChargePrefix).Get(hash[:])
	if bz == nil {
		return nil, nil
	}
	charge := new(state.FeeTokenCharge)
	if err := json.Unmarshal(bz, charge); err != nil {
		return nil, fmt.Errorf("decode fee-token charge: %w", err)
	}
	if err := sdk.ValidateDenom(charge.Denom); err != nil {
		return nil, err
	}
	if charge.Rate.IsNil() || !charge.Rate.IsPositive() {
		return nil, ErrFeeTokenRateInvalid
	}
	return charge, nil
}
