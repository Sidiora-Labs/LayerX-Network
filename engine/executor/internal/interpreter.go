package internal

import (
	"fmt"
	"math"
	"math/big"

	"github.com/ethereum/evmc/v12/bindings/go/evmc"
	"github.com/ethereum/go-ethereum/core/vm"
)

var _ vm.IEVMInterpreter = (*EVMInterpreter)(nil)

// EVMInterpreter is a custom interpreter that delegates execution to evmone via EVMC.
type EVMInterpreter struct {
	hostContext *HostContext
	evm         *vm.EVM
	readOnly    bool
}

func NewEVMInterpreter(hostContext *HostContext, evm *vm.EVM) *EVMInterpreter {
	return &EVMInterpreter{hostContext: hostContext, evm: evm}
}

// Run executes the contract code via evmone.
func (e *EVMInterpreter) Run(callOpCode vm.OpCode, contract *vm.Contract, input []byte, readOnly bool) ([]byte, error) {
	if contract == nil {
		return nil, fmt.Errorf("evmone execution requires a contract")
	}
	if contract.Gas > uint64(math.MaxInt64) {
		return nil, fmt.Errorf("evmone gas limit %d exceeds signed host limit", contract.Gas)
	}
	if err := e.validateHostContext(); err != nil {
		return nil, err
	}
	// Increment the call depth which is restricted to 1024
	e.evm.Depth++
	defer func() { e.evm.Depth-- }()
	depth := e.evm.Depth

	// For CREATE/CREATE2, the initcode is in contract.Code, not in input.
	// For regular calls, input contains the call data.
	codeToExecute := input
	if callOpCode == vm.CREATE || callOpCode == vm.CREATE2 {
		codeToExecute = contract.Code
	}

	// Make sure the readOnly is only set if we aren't in readOnly yet.
	// This also makes sure that the readOnly flag isn't removed for child calls.
	if readOnly && !e.readOnly {
		e.readOnly = true
		defer func() { e.readOnly = false }()
	}

	var static bool
	if callOpCode == vm.STATICCALL {
		static = true
	}

	var callKind evmc.CallKind
	switch callOpCode {
	case vm.STATICCALL:
		fallthrough
	case vm.CALL:
		callKind = evmc.Call
	case vm.DELEGATECALL:
		callKind = evmc.DelegateCall
	case vm.CREATE2:
		callKind = evmc.Create2
	case vm.CREATE:
		callKind = evmc.Create
	case vm.CALLCODE:
		callKind = evmc.CallCode
	default:
		return nil, fmt.Errorf("unsupported evmone call opcode %s", callOpCode)
	}

	// todo(pdrobnjak): sender and recipient might not be correctly propagated in case of DELEGATECALL
	sender := evmc.Address(contract.Caller())
	recipient := evmc.Address(contract.Address())

	// Keep adjustment state scoped to this invocation. Nested EVM calls receive
	// their own frame so a child cannot erase or duplicate its parent's charge.
	e.hostContext.BeginSstoreGasAdjustment()

	//nolint:gosec // gosec: safe gas conversion
	output, gasLeft, gasRefund, _, err := e.hostContext.Execute(callKind, recipient, sender, contract.Value().Bytes32(), codeToExecute,
		int64(contract.Gas), depth, static)
	sstoreAdjustment, sstoreOverflow := e.hostContext.EndSstoreGasAdjustment()
	if err != nil {
		return nil, err
	}
	if gasLeft < 0 || gasLeft > int64(contract.Gas) {
		return nil, fmt.Errorf("evmone returned invalid gas remainder %d for limit %d", gasLeft, contract.Gas)
	}
	if gasRefund < 0 {
		return nil, fmt.Errorf("evmone returned negative gas refund %d", gasRefund)
	}

	// Apply SSTORE gas adjustment for Pax's custom SSTORE cost.
	// evmone uses standard EIP-2200 gas (20k), but Pax may have a different cost.
	// The adjustment is tracked during SetStorage calls and applied here.
	// Adjustment can be positive (charge more) or negative (refund/reduce).
	if sstoreOverflow {
		return nil, fmt.Errorf("evmone SSTORE gas adjustment overflow")
	}
	if sstoreAdjustment != 0 {
		if sstoreAdjustment > 0 && gasLeft < sstoreAdjustment {
			return nil, vm.ErrOutOfGas
		}
		if sstoreAdjustment < 0 && gasLeft > math.MaxInt64+sstoreAdjustment {
			return nil, fmt.Errorf("evmone SSTORE gas adjustment exceeds signed host limit")
		}
		gasLeft -= sstoreAdjustment
		if gasLeft > int64(contract.Gas) {
			return nil, fmt.Errorf("evmone SSTORE adjustment returns %d gas from limit %d", gasLeft, contract.Gas)
		}
	}

	// Update the contract's gas to reflect what evmone consumed
	// This is critical for proper gas accounting!
	//nolint:gosec // safe conversion - gasLeft is always <= contract.Gas
	contract.Gas = uint64(gasLeft)

	// Apply gas refund to the EVM's refund counter
	//nolint:gosec // safe conversion
	e.evm.StateDB.AddRefund(uint64(gasRefund))

	return output, nil
}

func (e *EVMInterpreter) validateHostContext() error {
	context := e.evm.Context
	if context.BlockNumber == nil || !context.BlockNumber.IsInt64() || context.BlockNumber.Sign() < 0 {
		return fmt.Errorf("evmone requires a non-negative signed 64-bit block number")
	}
	if context.Time > uint64(math.MaxInt64) {
		return fmt.Errorf("evmone timestamp %d exceeds signed host limit", context.Time)
	}
	if context.GasLimit > uint64(math.MaxInt64) {
		return fmt.Errorf("evmone block gas limit %d exceeds signed host limit", context.GasLimit)
	}
	chainConfig := e.evm.ChainConfig()
	if chainConfig == nil || chainConfig.ChainID == nil || chainConfig.ChainID.Sign() <= 0 || chainConfig.ChainID.BitLen() > 256 {
		return fmt.Errorf("evmone requires a positive 256-bit chain ID")
	}
	if err := validateUnsignedWord("gas price", e.evm.GasPrice); err != nil {
		return err
	}
	if err := validateUnsignedWord("base fee", context.BaseFee); err != nil {
		return err
	}
	if err := validateUnsignedWord("blob base fee", context.BlobBaseFee); err != nil {
		return err
	}
	return nil
}

func validateUnsignedWord(name string, value *big.Int) error {
	if value != nil && (value.Sign() < 0 || value.BitLen() > 256) {
		return fmt.Errorf("evmone %s is outside the unsigned 256-bit range", name)
	}
	return nil
}

func (e *EVMInterpreter) ReadOnly() bool {
	return e.readOnly
}
