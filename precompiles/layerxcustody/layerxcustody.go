// Package layerxcustody is the precompile through which EVM accounts and
// contracts use the native LayerX custody module: deposits into the custody
// module account, proof-carrying withdrawals and forced exits out of it, and
// read access to custody state.
//
// No key authorises a release. requestWithdrawal and finaliseWithdrawal take a
// LayerX receipt, its inclusion proof, the batch header and the sequencer's
// header signature; requestForcedExit and executeForcedExit take a native
// state witness and the account authority's recipient signature. The module
// verifies all of it against its AnchorReader (the authorised sequencer keys
// and the finalized checkpoint roots) and pays from the module account through
// the bank keeper.
//
// Every state change is emitted both as an EVM log from this address and as a
// Cosmos typed event. The deposit log is LayerXVault's CustodyDeposit event.
//
// Gas is charged by the EVM from RequiredGas before Execute runs:
//
//	gas = BaseGas
//	    + GasPerByte      * len(calldata after the selector)
//	    + GasPerSignature * signatures(method)
//	    + GasPerProofNode * proofNodes(method, args)
//	    + GasPerWrite     * writes(method)
//
// signatures is 2 for requestWithdrawal and finaliseWithdrawal (receipt and
// batch header), 1 for requestForcedExit and executeForcedExit, 0 otherwise.
// proofNodes is len(proof)/32 for the withdrawal methods and len(witness)/32
// for the forced-exit methods. writes is the fixed number of state slots a
// method may touch: deposit and depositToken 8, requestWithdrawal and
// requestForcedExit 6, finaliseWithdrawal and executeForcedExit 12 (they may
// queue and pay in one call), views 0.
package layerxcustody

import (
	"embed"
	"errors"
	"fmt"
	"math/big"

	"github.com/ethereum/go-ethereum/accounts/abi"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/tracing"
	"github.com/ethereum/go-ethereum/core/vm"
	custodykeeper "github.com/sidiora-labs/paxeer-network/modules/layerxcustody/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/layerxcustody/types"
	pcommon "github.com/sidiora-labs/paxeer-network/precompiles/common"
	"github.com/sidiora-labs/paxeer-network/precompiles/utils"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

const (
	DepositMethod            = "deposit"
	DepositTokenMethod       = "depositToken"
	RequestWithdrawalMethod  = "requestWithdrawal"
	FinaliseWithdrawalMethod = "finaliseWithdrawal"
	RequestForcedExitMethod  = "requestForcedExit"
	ExecuteForcedExitMethod  = "executeForcedExit"
	DepositCountMethod       = "depositCount"
	DepositNonceMethod       = "depositNonce"
	GetDepositMethod         = "getDeposit"
	GetDepositByIndexMethod  = "getDepositByIndex"
	GetClaimMethod           = "getClaim"
	NullifierStatusMethod    = "nullifierStatus"
	GetAssetMethod           = "getAsset"
	AssetByPointerMethod     = "assetByPointer"
	NativeAssetIDMethod      = "nativeAssetId"
	ExitEligibleMethod       = "exitEligible"

	RegisterDepositRootMethod       = "registerDepositRoot"
	DepositRootAuthorityMethod      = "depositRootAuthority"
	DepositRootRegisteredMethod     = "depositRootRegistered"
	DepositRegistrationDigestMethod = "depositRegistrationDigest"

	CustodyDepositEvent        = "CustodyDeposit"
	ClaimQueuedEvent           = "ClaimQueued"
	ClaimFinalisedEvent        = "ClaimFinalised"
	CustodyReleaseEvent        = "CustodyRelease"
	EmergencyExitExecutedEvent = "EmergencyExitExecuted"
	DepositRootRegisteredEvent = "DepositRootRegistered"
)

const (
	LayerXCustodyAddress = types.CustodyAddress
	PrecompileName       = "layerxCustody"

	BaseGas         uint64 = 3000
	GasPerByte      uint64 = 16
	GasPerSignature uint64 = 4000
	GasPerProofNode uint64 = 100
	GasPerWrite     uint64 = 5000
)

//go:embed abi.json
var f embed.FS

// DepositRecord is the ABI tuple of a recorded deposit.
type DepositRecord struct {
	DepositId   [32]byte //nolint:revive,stylecheck
	Index       uint64
	Depositor   common.Address
	Beneficiary [32]byte
	AssetId     [32]byte //nolint:revive,stylecheck
	Denom       string
	Amount      *big.Int
	Nonce       uint64
	Height      uint64
}

// Claim is the ABI tuple of a withdrawal or forced-exit claim. Kind is 1 for
// a withdrawal and 2 for a forced exit; Status is 0 none, 1 pending, 2 paid,
// 3 cancelled.
type Claim struct {
	ClaimId      [32]byte //nolint:revive,stylecheck
	Kind         uint8
	Status       uint8
	Nullifier    [32]byte
	WithdrawalId [32]byte //nolint:revive,stylecheck
	Account      [32]byte
	AssetId      [32]byte //nolint:revive,stylecheck
	Denom        string
	Recipient    common.Address
	Amount       *big.Int
	BatchNumber  uint64
	Anchor       [32]byte
	AvailableAt  uint64
}

// Asset is the ABI tuple of an asset mapping and its custody accounting.
type Asset struct {
	AssetId        [32]byte //nolint:revive,stylecheck
	Denom          string
	Pointer        common.Address
	Enabled        bool
	Paused         bool
	MinimumDeposit *big.Int
	CustodyCap     *big.Int
	Custodied      *big.Int
	Released       *big.Int
	Pending        *big.Int
}

type PrecompileExecutor struct {
	abi        abi.ABI
	address    common.Address
	keeper     *custodykeeper.Keeper
	bankKeeper utils.BankKeeper
	evmKeeper  utils.EVMKeeper
}

func NewPrecompile(keepers utils.Keepers) (*pcommon.Precompile, error) {
	newAbi := pcommon.MustGetABI(f, "abi.json")
	p := &PrecompileExecutor{
		abi:        newAbi,
		address:    common.HexToAddress(LayerXCustodyAddress),
		keeper:     keepers.LayerXCustodyK(),
		bankKeeper: keepers.BankK(),
		evmKeeper:  keepers.EVMK(),
	}
	return pcommon.NewPrecompile(newAbi, p, p.address, PrecompileName).WithRevertReasons(), nil
}

// Signatures returns the Ed25519 verifications a method performs.
func Signatures(method string) uint64 {
	switch method {
	case RequestWithdrawalMethod, FinaliseWithdrawalMethod:
		return 2
	case RequestForcedExitMethod, ExecuteForcedExitMethod, RegisterDepositRootMethod:
		return 1
	default:
		return 0
	}
}

// Writes returns the fixed state-slot bound of a method.
func Writes(method string) uint64 {
	switch method {
	case DepositMethod, DepositTokenMethod:
		return 8
	case RequestWithdrawalMethod, RequestForcedExitMethod:
		return 6
	case FinaliseWithdrawalMethod, ExecuteForcedExitMethod:
		return 12
	case RegisterDepositRootMethod:
		return 2
	default:
		return 0
	}
}

// proofArgument is the index of the argument carrying Merkle path nodes.
func proofArgument(method string) (int, bool) {
	switch method {
	case RequestWithdrawalMethod, FinaliseWithdrawalMethod:
		return 1, true
	case RequestForcedExitMethod, ExecuteForcedExitMethod:
		return 0, true
	default:
		return 0, false
	}
}

// Gas is the documented formula over already-measured quantities.
func Gas(inputBytes, signatures, proofNodes, writes uint64) uint64 {
	return BaseGas + GasPerByte*inputBytes + GasPerSignature*signatures + GasPerProofNode*proofNodes + GasPerWrite*writes
}

func (p PrecompileExecutor) RequiredGas(input []byte, method *abi.Method) uint64 {
	nodes := uint64(0)
	if index, ok := proofArgument(method.Name); ok {
		if args, err := method.Inputs.Unpack(input); err == nil && len(args) > index {
			if proof, ok := args[index].([]byte); ok {
				nodes = uint64(len(proof)) / 32
			}
		}
	}
	return Gas(uint64(len(input)), Signatures(method.Name), nodes, Writes(method.Name))
}

func (p PrecompileExecutor) Execute(ctx sdk.Context, method *abi.Method, caller common.Address, _ common.Address, args []interface{}, value *big.Int, readOnly bool, evm *vm.EVM, hooks *tracing.Hooks) (ret []byte, err error) {
	defer func() {
		if recovered := recover(); recovered != nil {
			ret = nil
			err = fmt.Errorf("layerxcustody: %v", recovered)
		}
	}()
	if ctx.EVMPrecompileCalledFromDelegateCall() {
		return nil, errors.New("cannot delegatecall layerxCustody")
	}
	if method.Name != DepositMethod {
		if err = pcommon.ValidateNonPayable(value); err != nil {
			return nil, err
		}
	}
	if readOnly && Writes(method.Name) != 0 {
		return nil, errors.New("cannot call a layerxCustody state change from staticcall")
	}
	switch method.Name {
	case DepositMethod:
		return p.deposit(ctx, method, caller, args, value, evm, hooks)
	case DepositTokenMethod:
		return p.depositToken(ctx, method, caller, args, evm)
	case RequestWithdrawalMethod:
		return p.requestWithdrawal(ctx, method, args, evm)
	case FinaliseWithdrawalMethod:
		return p.finaliseWithdrawal(ctx, method, args, evm)
	case RequestForcedExitMethod:
		return p.requestForcedExit(ctx, method, args, evm)
	case ExecuteForcedExitMethod:
		return p.executeForcedExit(ctx, method, args, evm)
	case DepositCountMethod:
		return method.Outputs.Pack(p.keeper.GetDepositCount(ctx))
	case DepositNonceMethod:
		if err = pcommon.ValidateArgsLength(args, 2); err != nil {
			return nil, err
		}
		return method.Outputs.Pack(p.keeper.GetDepositNonce(ctx, args[0].(common.Address), args[1].([32]byte)))
	case GetDepositMethod:
		if err = pcommon.ValidateArgsLength(args, 1); err != nil {
			return nil, err
		}
		deposit, _ := p.keeper.GetDeposit(ctx, args[0].([32]byte))
		return method.Outputs.Pack(depositRecord(deposit))
	case GetDepositByIndexMethod:
		if err = pcommon.ValidateArgsLength(args, 1); err != nil {
			return nil, err
		}
		deposit, _ := p.keeper.GetDepositByIndex(ctx, args[0].(uint64))
		return method.Outputs.Pack(depositRecord(deposit))
	case GetClaimMethod:
		if err = pcommon.ValidateArgsLength(args, 1); err != nil {
			return nil, err
		}
		claim, _ := p.keeper.GetClaim(ctx, args[0].([32]byte))
		return method.Outputs.Pack(claimRecord(claim))
	case NullifierStatusMethod:
		if err = pcommon.ValidateArgsLength(args, 1); err != nil {
			return nil, err
		}
		nullifier, _ := p.keeper.GetNullifier(ctx, args[0].([32]byte))
		return method.Outputs.Pack(uint8(nullifier.Status)) //nolint:gosec
	case GetAssetMethod:
		if err = pcommon.ValidateArgsLength(args, 1); err != nil {
			return nil, err
		}
		asset, _ := p.keeper.GetAsset(ctx, args[0].([32]byte))
		return method.Outputs.Pack(p.assetRecord(ctx, asset))
	case AssetByPointerMethod:
		if err = pcommon.ValidateArgsLength(args, 1); err != nil {
			return nil, err
		}
		asset, _ := p.keeper.GetAssetByPointer(ctx, args[0].(common.Address))
		return method.Outputs.Pack(hash32(asset.AssetId))
	case NativeAssetIDMethod:
		asset, _ := p.keeper.GetAssetByDenom(ctx, sdk.MustGetBaseDenom())
		return method.Outputs.Pack(hash32(asset.AssetId))
	case ExitEligibleMethod:
		return method.Outputs.Pack(p.keeper.ExitEligible(ctx))
	case RegisterDepositRootMethod:
		return p.registerDepositRoot(ctx, method, caller, args, evm)
	case DepositRootAuthorityMethod:
		return method.Outputs.Pack(hash32(p.keeper.GetParams(ctx).DepositRootAuthority))
	case DepositRootRegisteredMethod:
		if err = pcommon.ValidateArgsLength(args, 1); err != nil {
			return nil, err
		}
		_, registered := p.keeper.GetDepositRoot(ctx, args[0].([32]byte))
		return method.Outputs.Pack(registered)
	case DepositRegistrationDigestMethod:
		if err = pcommon.ValidateArgsLength(args, 1); err != nil {
			return nil, err
		}
		registration, _ := p.keeper.GetDepositRoot(ctx, args[0].([32]byte))
		return method.Outputs.Pack(hash32(registration.Commitment))
	}
	return nil, fmt.Errorf("layerxcustody: unknown method %s", method.Name)
}

func hash32(text string) [32]byte {
	value, _ := types.ParseHash32(text)
	return value
}

func address(text string) common.Address {
	value, _ := types.ParseAddress(text)
	return value
}

func amount(text string) *big.Int {
	value, ok := new(big.Int).SetString(text, 10)
	if !ok {
		return new(big.Int)
	}
	return value
}

func depositRecord(d types.Deposit) DepositRecord {
	return DepositRecord{DepositId: hash32(d.DepositId), Index: d.Index, Depositor: address(d.Depositor),
		Beneficiary: hash32(d.Beneficiary), AssetId: hash32(d.AssetId), Denom: d.Denom, Amount: amount(d.Amount),
		Nonce: d.Nonce, Height: uint64(d.Height)} //nolint:gosec
}

func claimRecord(c types.Claim) Claim {
	return Claim{ClaimId: hash32(c.ClaimId), Kind: uint8(c.Kind), Status: uint8(c.Status), //nolint:gosec
		Nullifier: hash32(c.Nullifier), WithdrawalId: hash32(c.WithdrawalId), Account: hash32(c.Account),
		AssetId: hash32(c.AssetId), Denom: c.Denom, Recipient: address(c.Recipient), Amount: amount(c.Amount),
		BatchNumber: c.BatchNumber, Anchor: hash32(c.Anchor), AvailableAt: uint64(c.AvailableAt)} //nolint:gosec
}

func (p PrecompileExecutor) assetRecord(ctx sdk.Context, a types.AssetMapping) Asset {
	totals := p.keeper.GetTotals(ctx, hash32(a.AssetId))
	return Asset{AssetId: hash32(a.AssetId), Denom: a.Denom, Pointer: address(a.Pointer), Enabled: a.Enabled,
		Paused: a.Paused, MinimumDeposit: amount(a.MinimumDeposit), CustodyCap: amount(a.CustodyCap),
		Custodied: amount(totals.Custodied), Released: amount(totals.Released), Pending: amount(totals.Pending)}
}

// log emits one ABI event from the custody address: indexed values become
// topics in declaration order and the rest is ABI-encoded data.
func (p PrecompileExecutor) log(evm *vm.EVM, name string, topics []common.Hash, data ...interface{}) error {
	event, ok := p.abi.Events[name]
	if !ok {
		return fmt.Errorf("layerxcustody: unknown event %s", name)
	}
	packed, err := event.Inputs.NonIndexed().Pack(data...)
	if err != nil {
		return err
	}
	return pcommon.EmitEVMLog(evm, p.address, append([]common.Hash{event.ID}, topics...), packed)
}

// registerDepositRoot records a finalized checkpoint's deposit root for the
// account that submitted the checkpoint.
func (p PrecompileExecutor) registerDepositRoot(ctx sdk.Context, method *abi.Method, caller common.Address,
	args []interface{}, evm *vm.EVM) ([]byte, error) {
	if err := pcommon.ValidateArgsLength(args, 3); err != nil {
		return nil, err
	}
	registration, err := p.keeper.RegisterDepositRoot(ctx, p.evmKeeper.GetPaxAddressOrDefault(ctx, caller),
		args[0].([]byte), args[1].([]byte), args[2].([][32]byte))
	if err != nil {
		return nil, err
	}
	if err := p.log(evm, DepositRootRegisteredEvent,
		[]common.Hash{hash32(registration.CheckpointId), hash32(registration.DepositRoot)},
		hash32(registration.Commitment), custodykeeper.DepositRootEvidenceVersion); err != nil {
		return nil, err
	}
	return method.Outputs.Pack()
}

func addressTopic(value common.Address) common.Hash { return common.BytesToHash(value.Bytes()) }

func (p PrecompileExecutor) recordDeposit(ctx sdk.Context, method *abi.Method, caller common.Address,
	assetID, beneficiary [32]byte, value sdk.Int, evm *vm.EVM) ([]byte, error) {
	deposit, err := p.keeper.Deposit(ctx, caller, p.evmKeeper.GetPaxAddressOrDefault(ctx, caller), assetID, beneficiary, value)
	if err != nil {
		return nil, err
	}
	depositID := hash32(deposit.DepositId)
	if err := p.log(evm, CustodyDepositEvent, []common.Hash{depositID, assetID, addressTopic(caller)},
		beneficiary, value.BigInt(), deposit.Nonce); err != nil {
		return nil, err
	}
	return method.Outputs.Pack(depositID)
}

// deposit custodies the native coin. msg.value must be a whole number of base
// units: LayerX amounts are bank base units, and a wei remainder is refused.
func (p PrecompileExecutor) deposit(ctx sdk.Context, method *abi.Method, caller common.Address, args []interface{},
	value *big.Int, evm *vm.EVM, hooks *tracing.Hooks) ([]byte, error) {
	if err := pcommon.ValidateArgsLength(args, 1); err != nil {
		return nil, err
	}
	if value == nil || value.Sign() <= 0 {
		return nil, errors.New("layerxcustody: deposit requires a non-zero value")
	}
	asset, found := p.keeper.GetAssetByDenom(ctx, sdk.MustGetBaseDenom())
	if !found {
		return nil, types.ErrUnknownAsset
	}
	coin, err := pcommon.HandlePaymentUhpx(ctx, p.evmKeeper.GetPaxAddressOrDefault(ctx, p.address),
		p.evmKeeper.GetPaxAddressOrDefault(ctx, caller), value, p.bankKeeper, p.evmKeeper, hooks, evm.GetDepth())
	if err != nil {
		return nil, err
	}
	return p.recordDeposit(ctx, method, caller, hash32(asset.AssetId), args[0].([32]byte), coin.Amount, evm)
}

// depositToken custodies a bank denom addressed by its ERC20 pointer. The
// denom moves through the bank keeper from the caller's account; amount is in
// the denom's base units.
func (p PrecompileExecutor) depositToken(ctx sdk.Context, method *abi.Method, caller common.Address, args []interface{},
	evm *vm.EVM) ([]byte, error) {
	if err := pcommon.ValidateArgsLength(args, 3); err != nil {
		return nil, err
	}
	asset, found := p.keeper.GetAssetByPointer(ctx, args[0].(common.Address))
	if !found {
		return nil, types.ErrUnknownAsset
	}
	return p.recordDeposit(ctx, method, caller, hash32(asset.AssetId), args[2].([32]byte),
		sdk.NewIntFromBigInt(args[1].(*big.Int)), evm)
}

func withdrawalEvidence(args []interface{}) (custodykeeper.WithdrawalEvidence, error) {
	if err := pcommon.ValidateArgsLength(args, 4); err != nil {
		return custodykeeper.WithdrawalEvidence{}, err
	}
	evidence := custodykeeper.WithdrawalEvidence{Receipt: args[0].([]byte), Proof: args[1].([]byte),
		Header: args[2].([]byte), HeaderSignature: args[3].([]byte)}
	for _, part := range [][]byte{evidence.Receipt, evidence.Proof, evidence.Header} {
		if len(part) > types.MaxEvidenceBytes {
			return custodykeeper.WithdrawalEvidence{}, errors.New("layerxcustody: evidence exceeds the custody bound")
		}
	}
	return evidence, nil
}

func exitEvidence(args []interface{}) (custodykeeper.ExitEvidence, error) {
	if err := pcommon.ValidateArgsLength(args, 6); err != nil {
		return custodykeeper.ExitEvidence{}, err
	}
	evidence := custodykeeper.ExitEvidence{Witness: args[0].([]byte), BatchNumber: args[1].(uint64),
		Account: args[2].([32]byte), AssetID: args[3].([32]byte), Recipient: args[4].(common.Address),
		RecipientSignature: args[5].([]byte)}
	if len(evidence.Witness) > types.MaxEvidenceBytes {
		return custodykeeper.ExitEvidence{}, errors.New("layerxcustody: witness exceeds the custody bound")
	}
	return evidence, nil
}

func (p PrecompileExecutor) logQueued(evm *vm.EVM, claim types.Claim) error {
	return p.log(evm, ClaimQueuedEvent, []common.Hash{hash32(claim.ClaimId), hash32(claim.Nullifier), hash32(claim.Anchor)},
		hash32(claim.AssetId), address(claim.Recipient), amount(claim.Amount), uint64(claim.AvailableAt)) //nolint:gosec
}

func (p PrecompileExecutor) logPaid(evm *vm.EVM, result custodykeeper.ClaimResult) error {
	claim := result.Claim
	if result.Queued {
		if err := p.logQueued(evm, claim); err != nil {
			return err
		}
	}
	if claim.Kind == types.ClaimKind_CLAIM_KIND_FORCED_EXIT {
		if err := p.log(evm, EmergencyExitExecutedEvent,
			[]common.Hash{hash32(claim.ClaimId), hash32(claim.Nullifier), hash32(claim.Anchor)},
			hash32(claim.Account), hash32(claim.AssetId), address(claim.Recipient), amount(claim.Amount)); err != nil {
			return err
		}
	} else if err := p.log(evm, ClaimFinalisedEvent, []common.Hash{hash32(claim.ClaimId), hash32(claim.Nullifier)}); err != nil {
		return err
	}
	return p.log(evm, CustodyReleaseEvent,
		[]common.Hash{hash32(claim.ClaimId), hash32(claim.AssetId), addressTopic(address(claim.Recipient))},
		amount(claim.Amount), p.address)
}

func (p PrecompileExecutor) requestWithdrawal(ctx sdk.Context, method *abi.Method, args []interface{}, evm *vm.EVM) ([]byte, error) {
	evidence, err := withdrawalEvidence(args)
	if err != nil {
		return nil, err
	}
	claim, err := p.keeper.RequestWithdrawal(ctx, evidence)
	if err != nil {
		return nil, err
	}
	if err := p.logQueued(evm, claim); err != nil {
		return nil, err
	}
	return method.Outputs.Pack(hash32(claim.ClaimId), uint64(claim.AvailableAt)) //nolint:gosec
}

func (p PrecompileExecutor) finaliseWithdrawal(ctx sdk.Context, method *abi.Method, args []interface{}, evm *vm.EVM) ([]byte, error) {
	evidence, err := withdrawalEvidence(args)
	if err != nil {
		return nil, err
	}
	result, err := p.keeper.FinaliseWithdrawal(ctx, evidence)
	if err != nil {
		return nil, err
	}
	if err := p.logPaid(evm, result); err != nil {
		return nil, err
	}
	return method.Outputs.Pack(hash32(result.Claim.ClaimId))
}

func (p PrecompileExecutor) requestForcedExit(ctx sdk.Context, method *abi.Method, args []interface{}, evm *vm.EVM) ([]byte, error) {
	evidence, err := exitEvidence(args)
	if err != nil {
		return nil, err
	}
	claim, err := p.keeper.RequestForcedExit(ctx, evidence)
	if err != nil {
		return nil, err
	}
	if err := p.logQueued(evm, claim); err != nil {
		return nil, err
	}
	return method.Outputs.Pack(hash32(claim.ClaimId), uint64(claim.AvailableAt)) //nolint:gosec
}

func (p PrecompileExecutor) executeForcedExit(ctx sdk.Context, method *abi.Method, args []interface{}, evm *vm.EVM) ([]byte, error) {
	evidence, err := exitEvidence(args)
	if err != nil {
		return nil, err
	}
	result, err := p.keeper.ExecuteForcedExit(ctx, evidence)
	if err != nil {
		return nil, err
	}
	if err := p.logPaid(evm, result); err != nil {
		return nil, err
	}
	return method.Outputs.Pack(hash32(result.Claim.ClaimId))
}
