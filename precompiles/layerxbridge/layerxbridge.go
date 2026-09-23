// Package layerxbridge is the precompile through which EVM accounts use the
// native LayerX bridge module: bridgeIn mints an attested remote deposit,
// bridgeOut burns a bridged denom toward its remote vault, and views read the
// chain registry, attestor set, caps, pause and nullifiers.
//
// bridgeIn is permissionless: its authority is the attestor signatures over
// the PaxeerXVault deposit digest (modules/layerxbridge/ATTESTATION.md).
// Every state change is emitted both as an EVM log from this address and as a
// Cosmos event.
//
// Gas is charged by the EVM from RequiredGas before Execute runs:
//
//	gas = BaseGas
//	    + GasPerByte      * len(calldata after the selector)
//	    + GasPerSignature * signatures(method, args)
//	    + GasPerWrite     * writes(method)
//
// signatures is len(signatures) for bridgeIn and 0 otherwise. writes is the
// fixed number of state slots a method may touch: bridgeIn 8, bridgeOut 6,
// views 0.
package layerxbridge

import (
	"embed"
	"errors"
	"fmt"
	"math/big"

	"github.com/ethereum/go-ethereum/accounts/abi"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/tracing"
	"github.com/ethereum/go-ethereum/core/vm"
	bridgekeeper "github.com/sidiora-labs/paxeer-network/modules/layerxbridge/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/layerxbridge/types"
	pcommon "github.com/sidiora-labs/paxeer-network/precompiles/common"
	"github.com/sidiora-labs/paxeer-network/precompiles/utils"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

const (
	BridgeInMethod     = "bridgeIn"
	BridgeOutMethod    = "bridgeOut"
	GetChainMethod     = "getChain"
	GetAttestorsMethod = "getAttestors"
	GetCapMethod       = "getCap"
	IsPausedMethod     = "isPaused"
	IsNullifiedMethod  = "isNullified"

	BridgeInEvent  = "BridgeIn"
	BridgeOutEvent = "BridgeOut"
)

const (
	BridgeAddress  = types.BridgeAddress
	PrecompileName = "layerxBridge"

	BaseGas         uint64 = 3000
	GasPerByte      uint64 = 16
	GasPerSignature uint64 = 3000
	GasPerWrite     uint64 = 5000
)

//go:embed abi.json
var f embed.FS

// BridgeKeeper is the bridge keeper surface this precompile uses.
type BridgeKeeper interface {
	BridgeIn(ctx sdk.Context, in types.BridgeIn, signatures [][]byte) (bridgekeeper.BridgeInResult, error)
	BridgeOut(ctx sdk.Context, sender common.Address, chainID uint64, asset types.Address20, amount *big.Int,
		recipient types.Address20) (bridgekeeper.BridgeOutResult, error)
	GetChain(ctx sdk.Context, chainID uint64) (types.Chain, bool)
	GetAttestorSet(ctx sdk.Context) types.AttestorSet
	GetAsset(ctx sdk.Context, chainID uint64, asset types.Address20) (types.BridgedAsset, bool)
	GetCap(ctx sdk.Context, denom string) (types.Cap, bool)
	InFlight(ctx sdk.Context, denom string) sdk.Int
	IsPaused(ctx sdk.Context) bool
	IsNullified(ctx sdk.Context, nullifier types.Nullifier) bool
}

type PrecompileExecutor struct {
	abi     abi.ABI
	address common.Address
	keeper  BridgeKeeper
}

// NewPrecompile builds the precompile from the application's precompile
// keepers, which must also provide LayerXBridgeK.
func NewPrecompile(keepers utils.Keepers) (*pcommon.Precompile, error) {
	bridge := keepers.LayerXBridgeK()
	if bridge == nil {
		return nil, errors.New("layerxbridge: the precompile keepers provide no LayerXBridgeK")
	}
	return NewPrecompileWithKeeper(bridge), nil
}

// NewPrecompileWithKeeper builds the precompile over an explicit bridge keeper.
func NewPrecompileWithKeeper(bridge BridgeKeeper) *pcommon.Precompile {
	newAbi := pcommon.MustGetABI(f, "abi.json")
	p := &PrecompileExecutor{
		abi:     newAbi,
		address: common.HexToAddress(BridgeAddress),
		keeper:  bridge,
	}
	return pcommon.NewPrecompile(newAbi, p, p.address, PrecompileName).WithRevertReasons()
}

// Writes returns the fixed state-slot bound of a method.
func Writes(method string) uint64 {
	switch method {
	case BridgeInMethod:
		return 8
	case BridgeOutMethod:
		return 6
	default:
		return 0
	}
}

// Gas is the documented formula over already-measured quantities.
func Gas(inputBytes, signatures, writes uint64) uint64 {
	return BaseGas + GasPerByte*inputBytes + GasPerSignature*signatures + GasPerWrite*writes
}

func (p PrecompileExecutor) RequiredGas(input []byte, method *abi.Method) uint64 {
	signatures := uint64(0)
	if method.Name == BridgeInMethod {
		if args, err := method.Inputs.Unpack(input); err == nil && len(args) == 8 {
			if list, ok := args[7].([][]byte); ok {
				signatures = uint64(len(list))
			}
		}
	}
	return Gas(uint64(len(input)), signatures, Writes(method.Name))
}

func (p PrecompileExecutor) Execute(ctx sdk.Context, method *abi.Method, caller common.Address, _ common.Address,
	args []interface{}, value *big.Int, readOnly bool, evm *vm.EVM, _ *tracing.Hooks) (ret []byte, err error) {
	defer func() {
		if recovered := recover(); recovered != nil {
			ret = nil
			err = fmt.Errorf("layerxbridge: %v", recovered)
		}
	}()
	if ctx.EVMPrecompileCalledFromDelegateCall() {
		return nil, errors.New("cannot delegatecall layerxBridge")
	}
	if err = pcommon.ValidateNonPayable(value); err != nil {
		return nil, err
	}
	if readOnly && Writes(method.Name) != 0 {
		return nil, errors.New("cannot call a layerxBridge state change from staticcall")
	}
	switch method.Name {
	case BridgeInMethod:
		return p.bridgeIn(ctx, method, args, evm)
	case BridgeOutMethod:
		return p.bridgeOut(ctx, method, caller, args, evm)
	case GetChainMethod:
		if err = pcommon.ValidateArgsLength(args, 1); err != nil {
			return nil, err
		}
		chain, found := p.keeper.GetChain(ctx, args[0].(uint64))
		return method.Outputs.Pack(found, common.Address(chain.Vault), chain.FinalityDepth, chain.Enabled)
	case GetAttestorsMethod:
		set := p.keeper.GetAttestorSet(ctx)
		signers := make([]common.Address, len(set.Attestors))
		bonds := make([]*big.Int, len(set.Attestors))
		for i, attestor := range set.Attestors {
			signers[i] = common.Address(attestor.Signer)
			bonds[i] = attestor.Bond.BigInt()
		}
		return method.Outputs.Pack(signers, bonds, set.Threshold)
	case GetCapMethod:
		if err = pcommon.ValidateArgsLength(args, 2); err != nil {
			return nil, err
		}
		asset, found := p.keeper.GetAsset(ctx, args[0].(uint64), types.Address20(args[1].(common.Address)))
		if !found {
			return method.Outputs.Pack("", new(big.Int), new(big.Int), new(big.Int))
		}
		limits, _ := p.keeper.GetCap(ctx, asset.Denom)
		return method.Outputs.Pack(asset.Denom, bigOrZero(limits.MaxInFlight), bigOrZero(limits.MaxPerTx),
			p.keeper.InFlight(ctx, asset.Denom).BigInt())
	case IsPausedMethod:
		return method.Outputs.Pack(p.keeper.IsPaused(ctx))
	case IsNullifiedMethod:
		if err = pcommon.ValidateArgsLength(args, 3); err != nil {
			return nil, err
		}
		return method.Outputs.Pack(p.keeper.IsNullified(ctx, types.Nullifier{ChainID: args[0].(uint64),
			TxHash: args[1].([32]byte), LogIndex: args[2].(uint64)}))
	}
	return nil, fmt.Errorf("layerxbridge: unknown method %s", method.Name)
}

func bigOrZero(value sdk.Int) *big.Int {
	if value.IsNil() {
		return new(big.Int)
	}
	return value.BigInt()
}

// log emits one ABI event from the bridge address: indexed values become
// topics in declaration order and the rest is ABI-encoded data.
func (p PrecompileExecutor) log(evm *vm.EVM, name string, topics []common.Hash, data ...interface{}) error {
	event, ok := p.abi.Events[name]
	if !ok {
		return fmt.Errorf("layerxbridge: unknown event %s", name)
	}
	packed, err := event.Inputs.NonIndexed().Pack(data...)
	if err != nil {
		return err
	}
	return pcommon.EmitEVMLog(evm, p.address, append([]common.Hash{event.ID}, topics...), packed)
}

func uint64Topic(value uint64) common.Hash { return common.BigToHash(new(big.Int).SetUint64(value)) }

func addressTopic(value common.Address) common.Hash { return common.BytesToHash(value.Bytes()) }

func (p PrecompileExecutor) bridgeIn(ctx sdk.Context, method *abi.Method, args []interface{}, evm *vm.EVM) ([]byte, error) {
	if err := pcommon.ValidateArgsLength(args, 8); err != nil {
		return nil, err
	}
	in := types.BridgeIn{
		ChainID:   args[0].(uint64),
		Vault:     types.Address20(args[1].(common.Address)),
		TxHash:    args[2].([32]byte),
		LogIndex:  args[3].(uint64),
		Recipient: args[4].([32]byte),
		Asset:     types.Address20(args[5].(common.Address)),
		Amount:    args[6].(*big.Int),
	}
	result, err := p.keeper.BridgeIn(ctx, in, args[7].([][]byte))
	if err != nil {
		return nil, err
	}
	recipient, _ := types.RecipientAddress(in.Recipient)
	if err := p.log(evm, BridgeInEvent,
		[]common.Hash{uint64Topic(in.ChainID), common.Hash(in.TxHash), addressTopic(common.Address(recipient))},
		in.LogIndex, common.Address(in.Asset), result.Amount.BigInt(), result.Denom); err != nil {
		return nil, err
	}
	return method.Outputs.Pack(result.Denom)
}

func (p PrecompileExecutor) bridgeOut(ctx sdk.Context, method *abi.Method, caller common.Address, args []interface{},
	evm *vm.EVM) ([]byte, error) {
	if err := pcommon.ValidateArgsLength(args, 4); err != nil {
		return nil, err
	}
	chainID := args[0].(uint64)
	asset := args[1].(common.Address)
	amount := args[2].(*big.Int)
	recipient := args[3].(common.Address)
	result, err := p.keeper.BridgeOut(ctx, caller, chainID, types.Address20(asset), amount, types.Address20(recipient))
	if err != nil {
		return nil, err
	}
	if err := p.log(evm, BridgeOutEvent, []common.Hash{uint64Topic(chainID), addressTopic(asset), uint64Topic(result.Nonce)},
		amount, recipient); err != nil {
		return nil, err
	}
	return method.Outputs.Pack(result.Nonce)
}
