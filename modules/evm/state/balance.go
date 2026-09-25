package state

import (
	"fmt"
	"math/big"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/tracing"
	"github.com/holiman/uint256"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

var ZeroInt = uint256.NewInt(0)

func (s *DBImpl) SubBalance(evmAddr common.Address, amtUint256 *uint256.Int, reason tracing.BalanceChangeReason) uint256.Int {
	if s.feeTokenCharge != nil && evmAddr == s.feeTokenCharge.Payer && reason == tracing.BalanceDecreaseGasBuy {
		s.gasBought = true
		return s.moveFeeToken(evmAddr, amtUint256, true)
	}
	s.k.PrepareReplayedAddr(s.ctx, evmAddr)
	amt := amtUint256.ToBig()
	if amt.Sign() == 0 {
		return *ZeroInt
	}
	if amt.Sign() < 0 {
		return s.AddBalance(evmAddr, new(uint256.Int).Neg(amtUint256), reason)
	}

	ctx := s.ctx
	var oldBalance *uint256.Int
	if s.logger != nil && s.logger.OnBalanceChange != nil {
		oldBalance = s.GetBalance(evmAddr)
	}

	// this avoids emitting cosmos events for ephemeral bookkeeping transfers like send_native
	if s.eventsSuppressed {
		ctx = ctx.WithEventManager(sdk.NewEventManager())
	}

	// Hook for mock balances (no-op in production builds)
	s.ensureSufficientBalance(evmAddr, amt)

	uhpx, wei := SplitUhpxWeiAmount(amt)
	addr := s.getPaxAddress(evmAddr)
	err := s.k.BankKeeper().SubUnlockedCoins(ctx, addr, sdk.NewCoins(sdk.NewCoin(s.k.GetBaseDenom(s.ctx), uhpx)), true)
	if err != nil {
		s.err = err
		return *ZeroInt
	}
	err = s.k.BankKeeper().SubWei(ctx, addr, wei)
	if err != nil {
		s.err = err
		return *ZeroInt
	}

	if s.logger != nil && s.logger.OnBalanceChange != nil && oldBalance != nil {
		newBalance := s.GetBalance(evmAddr).ToBig()
		oldBalance := new(big.Int).Add(newBalance, amt)
		s.logger.OnBalanceChange(evmAddr, oldBalance, newBalance, reason)
	}

	surplus := sdk.NewIntFromBigInt(amt)
	s.tempState.surplus = s.tempState.surplus.Add(surplus)
	s.journal = append(s.journal, &surplusChange{delta: surplus})
	return *ZeroInt
}

func (s *DBImpl) AddBalance(evmAddr common.Address, amtUint256 *uint256.Int, reason tracing.BalanceChangeReason) uint256.Int {
	if s.feeTokenCharge != nil && ((evmAddr == s.feeTokenCharge.Payer && reason == tracing.BalanceIncreaseGasReturn) || (evmAddr == s.coinbaseEvmAddress && reason == tracing.BalanceIncreaseRewardTransactionFee)) {
		return s.moveFeeToken(evmAddr, amtUint256, false)
	}
	s.k.PrepareReplayedAddr(s.ctx, evmAddr)
	amt := amtUint256.ToBig()
	if amt.Sign() == 0 {
		return *ZeroInt
	}
	if amt.Sign() < 0 {
		return s.SubBalance(evmAddr, new(uint256.Int).Neg(amtUint256), reason)
	}

	ctx := s.ctx
	var oldBalance *uint256.Int
	if s.logger != nil && s.logger.OnBalanceChange != nil {
		oldBalance = s.GetBalance(evmAddr)
	}
	// this avoids emitting cosmos events for ephemeral bookkeeping transfers like send_native
	if s.eventsSuppressed {
		ctx = ctx.WithEventManager(sdk.NewEventManager())
	}

	uhpx, wei := SplitUhpxWeiAmount(amt)
	addr := s.getPaxAddress(evmAddr)
	err := s.k.BankKeeper().AddCoins(ctx, addr, sdk.NewCoins(sdk.NewCoin(s.k.GetBaseDenom(s.ctx), uhpx)), true)
	if err != nil {
		s.err = err
		return *ZeroInt
	}
	err = s.k.BankKeeper().AddWei(ctx, addr, wei)
	if err != nil {
		s.err = err
		return *ZeroInt
	}

	if s.logger != nil && s.logger.OnBalanceChange != nil && oldBalance != nil {
		newBalance := s.GetBalance(evmAddr).ToBig()
		oldBalance := new(big.Int).Sub(newBalance, amt)
		s.logger.OnBalanceChange(evmAddr, oldBalance, newBalance, reason)
	}

	surplus := sdk.NewIntFromBigInt(amt).Neg()
	s.tempState.surplus = s.tempState.surplus.Add(surplus)
	s.journal = append(s.journal, &surplusChange{delta: surplus})
	return *ZeroInt
}

func (s *DBImpl) GetBalance(evmAddr common.Address) *uint256.Int {
	s.k.PrepareReplayedAddr(s.ctx, evmAddr)
	// Hook for mock balances (no-op in production builds)
	s.ensureMinimumBalance(evmAddr)
	paxAddr := s.getPaxAddress(evmAddr)
	balance := s.k.GetBalance(s.ctx, paxAddr)
	// Fee-token buying power augments the payer only before BuyGas; execution sees the real network-coin balance.
	if s.feeTokenCharge != nil && !s.gasBought && evmAddr == s.feeTokenCharge.Payer {
		amount := s.k.BankKeeper().SpendableCoins(s.ctx, paxAddr).AmountOf(s.feeTokenCharge.Denom)
		wei, err := s.k.ConvertFeeFromDenom(amount, s.feeTokenCharge.Rate, false)
		if err != nil {
			s.err = err
			return uint256.NewInt(0)
		}
		balance = new(big.Int).Add(balance, wei.BigInt())
		if balance.BitLen() > 256 {
			s.err = fmt.Errorf("fee-token buying power exceeds 256 bits")
			return uint256.NewInt(0)
		}
	}
	res, overflow := uint256.FromBig(balance)
	if overflow {
		panic("balance overflow")
	}
	if res == nil {
		return uint256.NewInt(0)
	}
	return res
}

// should only be called during simulation
func (s *DBImpl) SetBalance(evmAddr common.Address, amtUint256 *uint256.Int, reason tracing.BalanceChangeReason) {
	if !s.simulation {
		panic("should never call SetBalance in a non-simulation setting")
	}
	amt := amtUint256.ToBig()
	paxAddr := s.getPaxAddress(evmAddr)
	moduleAddr := s.k.AccountKeeper().GetModuleAddress(types.ModuleName)
	s.send(paxAddr, moduleAddr, s.GetBalance(evmAddr).ToBig())
	if s.err != nil {
		panic(s.err)
	}
	uhpx, _ := SplitUhpxWeiAmount(amt)
	coinsAmt := sdk.NewCoins(sdk.NewCoin(s.k.GetBaseDenom(s.ctx), uhpx.Add(sdk.OneInt())))
	if err := s.k.BankKeeper().MintCoins(s.ctx, types.ModuleName, coinsAmt); err != nil {
		panic(err)
	}
	s.send(moduleAddr, paxAddr, amt)
	if s.err != nil {
		panic(s.err)
	}
}

func (s *DBImpl) getPaxAddress(evmAddr common.Address) sdk.AccAddress {
	if s.coinbaseEvmAddress.Cmp(evmAddr) == 0 {
		return s.coinbaseAddress
	}
	return s.k.GetPaxAddressOrDefault(s.ctx, evmAddr)
}

func (s *DBImpl) send(from sdk.AccAddress, to sdk.AccAddress, amt *big.Int) {
	uhpx, wei := SplitUhpxWeiAmount(amt)
	err := s.k.BankKeeper().SendCoinsAndWei(s.ctx, from, to, uhpx, wei)
	if err != nil {
		s.err = err
	}
}

type FeeTokenCharge struct {
	Payer common.Address
	Denom string
	Rate  sdk.Dec
}

func (s *DBImpl) SetFeeTokenCharge(charge *FeeTokenCharge, gasBought bool) {
	s.feeTokenCharge = nil
	if charge != nil {
		copied := *charge
		s.feeTokenCharge = &copied
	}
	s.gasBought = gasBought
}

func (s *DBImpl) moveFeeToken(evmAddr common.Address, amount *uint256.Int, debit bool) uint256.Int {
	converted, err := s.k.ConvertFeeToDenom(sdk.NewIntFromBigInt(amount.ToBig()), s.feeTokenCharge.Rate, true)
	if err != nil {
		s.err = err
		return *ZeroInt
	}
	if converted.IsZero() {
		return *ZeroInt
	}
	ctx := s.ctx
	if s.eventsSuppressed {
		ctx = ctx.WithEventManager(sdk.NewEventManager())
	}
	coins := sdk.NewCoins(sdk.NewCoin(s.feeTokenCharge.Denom, converted))
	if debit {
		err = s.k.BankKeeper().SubUnlockedCoins(ctx, s.getPaxAddress(evmAddr), coins, true)
	} else {
		err = s.k.BankKeeper().AddCoins(ctx, s.getPaxAddress(evmAddr), coins, true)
	}
	if err != nil {
		s.err = err
	}
	return *ZeroInt
}
