package feetoken

import (
	"embed"
	"errors"
	"fmt"
	"math/big"

	"github.com/ethereum/go-ethereum/accounts/abi"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/tracing"
	"github.com/ethereum/go-ethereum/core/vm"
	pcommon "github.com/sidiora-labs/paxeer-network/precompiles/common"
	"github.com/sidiora-labs/paxeer-network/precompiles/utils"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

const (
	SetFeeDenomMethod          = "setFeeDenom"
	GetFeeDenomMethod          = "getFeeDenom"
	ClearFeeDenomMethod        = "clearFeeDenom"
	FeeTokenAddress            = "0x0000000000000000000000000000000000001018"
	PrecompileName             = "feeToken"
	BaseGas             uint64 = 3000
	GasPerByte          uint64 = 16
	GasPerWrite         uint64 = 5000
)

//go:embed abi.json
var f embed.FS

type FeeTokenKeeper interface {
	GetAccountFeeDenom(sdk.Context, common.Address) string
	SetAccountFeeDenom(sdk.Context, common.Address, string) error
	ClearAccountFeeDenom(sdk.Context, common.Address)
}

type PrecompileExecutor struct {
	keeper FeeTokenKeeper
}

func NewPrecompile(keepers utils.Keepers) (*pcommon.Precompile, error) {
	keeper, ok := keepers.EVMK().(FeeTokenKeeper)
	if !ok || keeper == nil {
		return nil, errors.New("feetoken: the precompile keepers provide no fee-token EVM keeper")
	}
	return NewPrecompileWithKeeper(keeper), nil
}

func NewPrecompileWithKeeper(keeper FeeTokenKeeper) *pcommon.Precompile {
	newABI := pcommon.MustGetABI(f, "abi.json")
	return pcommon.NewPrecompile(newABI, &PrecompileExecutor{keeper: keeper},
		common.HexToAddress(FeeTokenAddress), PrecompileName).WithRevertReasons()
}

func (PrecompileExecutor) IsTransaction(method string) bool {
	switch method {
	case SetFeeDenomMethod, ClearFeeDenomMethod:
		return true
	default:
		return false
	}
}

func (p PrecompileExecutor) RequiredGas(input []byte, method *abi.Method) uint64 {
	gas := BaseGas + GasPerByte*uint64(len(input))
	if p.IsTransaction(method.Name) {
		gas += GasPerWrite
	}
	return gas
}

func (p PrecompileExecutor) Execute(ctx sdk.Context, method *abi.Method, caller common.Address, _ common.Address,
	args []interface{}, value *big.Int, readOnly bool, _ *vm.EVM, _ *tracing.Hooks) (ret []byte, err error) {
	if ctx.EVMPrecompileCalledFromDelegateCall() {
		return nil, errors.New("cannot delegatecall feeToken")
	}
	if err = pcommon.ValidateNonPayable(value); err != nil {
		return nil, err
	}
	if readOnly && p.IsTransaction(method.Name) {
		return nil, errors.New("cannot call a feeToken state change from staticcall")
	}
	if p.keeper == nil {
		return nil, errors.New("feetoken: fee-token EVM keeper is unavailable")
	}
	switch method.Name {
	case SetFeeDenomMethod:
		if err = pcommon.ValidateArgsLength(args, 1); err != nil {
			return nil, err
		}
		if err = p.keeper.SetAccountFeeDenom(ctx, caller, args[0].(string)); err != nil {
			return nil, err
		}
		return method.Outputs.Pack()
	case GetFeeDenomMethod:
		if err = pcommon.ValidateArgsLength(args, 1); err != nil {
			return nil, err
		}
		return method.Outputs.Pack(p.keeper.GetAccountFeeDenom(ctx, args[0].(common.Address)))
	case ClearFeeDenomMethod:
		if err = pcommon.ValidateArgsLength(args, 0); err != nil {
			return nil, err
		}
		p.keeper.ClearAccountFeeDenom(ctx, caller)
		return method.Outputs.Pack()
	default:
		return nil, fmt.Errorf("feetoken: unknown method %s", method.Name)
	}
}
