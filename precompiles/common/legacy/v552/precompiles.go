package v552

import (
	"errors"
	"fmt"
	"math/big"

	"github.com/ethereum/go-ethereum/accounts/abi"
	"github.com/ethereum/go-ethereum/core/tracing"
	"github.com/ethereum/go-ethereum/core/vm"
	"github.com/sidiora-labs/paxeer-network/modules/evm/state"
	"github.com/sidiora-labs/paxeer-network/precompiles/utils"
	storetypes "github.com/sidiora-labs/paxeer-network/sdk/store/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

const UnknownMethodCallGas uint64 = 3000

type Contexter interface {
	Ctx() sdk.Context
}

type Precompile struct {
	abi.ABI
}

func (p Precompile) RequiredGas(input []byte, isTransaction bool) uint64 {
	argsBz := input[4:] // first four bytes are method ID

	if isTransaction {
		return storetypes.KVGasConfig().WriteCostFlat + (storetypes.KVGasConfig().WriteCostPerByte * uint64(len(argsBz)))
	}

	return storetypes.KVGasConfig().ReadCostFlat + (storetypes.KVGasConfig().ReadCostPerByte * uint64(len(argsBz)))
}

func (p Precompile) Prepare(evm *vm.EVM, input []byte) (sdk.Context, *abi.Method, []interface{}, error) {
	ctxer := state.GetDBImpl(evm.StateDB)
	if ctxer == nil {
		return sdk.Context{}, nil, nil, errors.New("cannot get context from EVM")
	}
	methodID, err := ExtractMethodID(input)
	if err != nil {
		return sdk.Context{}, nil, nil, err
	}
	method, err := p.MethodById(methodID)
	if err != nil {
		return sdk.Context{}, nil, nil, err
	}

	argsBz := input[4:]
	args, err := method.Inputs.Unpack(argsBz)
	if err != nil {
		return sdk.Context{}, nil, nil, err
	}

	return ctxer.Ctx(), method, args, nil
}

func (p Precompile) GetABI() abi.ABI {
	return p.ABI
}

func ValidateArgsLength(args []interface{}, length int) error {
	if len(args) != length {
		return fmt.Errorf("expected %d arguments but got %d", length, len(args))
	}

	return nil
}

func ValidateNonPayable(value *big.Int) error {
	if value != nil && value.Sign() != 0 {
		return errors.New("sending funds to a non-payable function")
	}

	return nil
}

func HandlePaymentUhpx(ctx sdk.Context, precompileAddr sdk.AccAddress, payer sdk.AccAddress, value *big.Int, bankKeeper utils.BankKeeper, evmKeeper utils.EVMKeeper, hooks *tracing.Hooks, depth int) (sdk.Coin, error) {
	uhpx, wei := state.SplitUhpxWeiAmount(value)
	if !wei.IsZero() {
		return sdk.Coin{}, fmt.Errorf("selected precompile function does not allow payment with non-zero wei remainder: received %s", value)
	}
	coin := sdk.NewCoin(sdk.MustGetBaseDenom(), uhpx)
	// refund payer because the following precompile logic will debit the payments from payer's account
	// this creates a new event manager to avoid surfacing these as cosmos events
	if err := bankKeeper.SendCoins(ctx.WithEventManager(sdk.NewEventManager()), precompileAddr, payer, sdk.NewCoins(coin)); err != nil {
		return sdk.Coin{}, err
	}
	if hooks != nil {
		newCtx := ctx.WithGasMeter(sdk.NewInfiniteGasMeterWithMultiplier(ctx))
		if hooks.OnEnter != nil {
			hooks.OnEnter(depth+1, byte(vm.CALL), evmKeeper.GetEVMAddressOrDefault(newCtx, precompileAddr), evmKeeper.GetEVMAddressOrDefault(newCtx, payer), []byte{}, GetRemainingGas(newCtx, evmKeeper), value)
		}
		if hooks.OnExit != nil {
			hooks.OnExit(depth+1, []byte{}, 0, nil, false)
		}
	}
	return coin, nil
}

func HandlePaymentUhpxWei(ctx sdk.Context, precompileAddr sdk.AccAddress, payer sdk.AccAddress, value *big.Int, bankKeeper utils.BankKeeper, evmKeeper utils.EVMKeeper, hooks *tracing.Hooks, depth int) (sdk.Int, sdk.Int, error) {
	uhpx, wei := state.SplitUhpxWeiAmount(value)
	// refund payer because the following precompile logic will debit the payments from payer's account
	// this creates a new event manager to avoid surfacing these as cosmos events
	if err := bankKeeper.SendCoinsAndWei(ctx.WithEventManager(sdk.NewEventManager()), precompileAddr, payer, uhpx, wei); err != nil {
		return sdk.Int{}, sdk.Int{}, err
	}
	if hooks != nil {
		newCtx := ctx.WithGasMeter(sdk.NewInfiniteGasMeterWithMultiplier(ctx))
		if hooks.OnEnter != nil {
			hooks.OnEnter(depth+1, byte(vm.CALL), evmKeeper.GetEVMAddressOrDefault(newCtx, precompileAddr), evmKeeper.GetEVMAddressOrDefault(newCtx, payer), []byte{}, GetRemainingGas(newCtx, evmKeeper), value)
		}
		if hooks.OnExit != nil {
			hooks.OnExit(depth+1, []byte{}, 0, nil, false)
		}
	}
	return uhpx, wei, nil
}

/*
*
pax gas = evm gas * multiplier
pax gas price = fee / pax gas = fee / (evm gas * multiplier) = evm gas / multiplier
*/
func GetRemainingGas(ctx sdk.Context, evmKeeper utils.EVMKeeper) uint64 {
	gasMultipler := evmKeeper.GetPriorityNormalizer(ctx)
	paxGasRemaining := ctx.GasMeter().Limit() - ctx.GasMeter().GasConsumedToLimit()
	return sdk.NewDecFromInt(sdk.NewIntFromUint64(paxGasRemaining)).Quo(gasMultipler).TruncateInt().Uint64()
}

func ExtractMethodID(input []byte) ([]byte, error) {
	// Check if the input has at least the length needed for methodID
	if len(input) < 4 {
		return nil, errors.New("input too short to extract method ID")
	}
	return input[:4], nil
}
