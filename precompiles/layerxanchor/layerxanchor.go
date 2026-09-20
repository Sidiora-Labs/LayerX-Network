// Package layerxanchor is the precompile over the native LayerX anchor module:
// checkpoint submission and finality, availability attestations, guarantor
// bonds, trustless equivocation slashing and authority-arbitrated challenges.
//
// Gas is charged by the EVM from RequiredGas before Execute runs:
//
//	gas = base(method)
//	    + GasPerByte      * len(calldata after the selector)
//	    + GasPerSignature * signatures(method, args)
//	    + GasPerProofNode * proofNodes(method, args)
//
// base is ViewBaseGas for views and WriteBaseGas for state changes.
// signatures is 1 + the certificate's attestation count for submitCheckpoint
// (one Ed25519 header signature and one secp256k1 recovery per guarantor),
// 1 for submitAvailabilityAttestation and 2 for submitEquivocation.
// proofNodes is len(validity proof)/32 for submitCheckpoint, the 32-byte
// blocks hashed into the checkpoint identifier. Both quantities are read from
// the certificate's length prefixes before anything is verified; a certificate
// too short to carry them is charged the 32-attestation maximum and reverts.
package layerxanchor

import (
	"embed"
	"encoding/binary"
	"errors"
	"fmt"
	"math/big"

	"github.com/ethereum/go-ethereum/accounts/abi"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/tracing"
	"github.com/ethereum/go-ethereum/core/vm"
	"github.com/sidiora-labs/paxeer-network/layerxproof/codec"
	anchortypes "github.com/sidiora-labs/paxeer-network/modules/layerxanchor/types"
	pcommon "github.com/sidiora-labs/paxeer-network/precompiles/common"
	"github.com/sidiora-labs/paxeer-network/precompiles/utils"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

const (
	SubmitCheckpointMethod          = "submitCheckpoint"
	SubmitAvailabilityMethod        = "submitAvailabilityAttestation"
	FinalizeMethod                  = "finalize"
	RegisterGuarantorMethod         = "registerGuarantor"
	IncreaseBondMethod              = "increaseBond"
	BeginUnbondMethod               = "beginUnbond"
	CompleteUnbondMethod            = "completeUnbond"
	SubmitEquivocationMethod        = "submitEquivocation"
	OpenChallengeMethod             = "openChallenge"
	ResolveChallengeMethod          = "resolveChallenge"
	ActivateGuarantorMethod         = "activateGuarantor"
	SetSequencerAuthorizationMethod = "setSequencerAuthorization"
	LatestFinalizedMethod           = "latestFinalized"
	CheckpointMethod                = "checkpoint"
	FinalizedStateRootMethod        = "finalizedStateRoot"
	FinalizedReceiptRootMethod      = "finalizedReceiptRoot"
	GuarantorMethod                 = "guarantor"
	ThresholdMethod                 = "threshold"
	StatusOfMethod                  = "statusOf"
)

const (
	LayerXAnchorAddress = "0x0000000000000000000000000000000000001014"
	PrecompileName      = "layerxAnchor"

	ViewBaseGas     uint64 = 3000
	WriteBaseGas    uint64 = 30000
	GasPerByte      uint64 = 16
	GasPerSignature uint64 = 4000
	GasPerProofNode uint64 = 100
)

//go:embed abi.json
var f embed.FS

// CheckpointView is the ABI tuple returned by checkpoint(uint64).
type CheckpointView struct {
	BatchNumber          uint64
	CheckpointId         [32]byte //nolint:revive,stylecheck
	HeaderDigest         [32]byte
	Epoch                uint64
	FirstSequence        uint64
	LastSequence         uint64
	PreviousStateRoot    [32]byte
	StateRoot            [32]byte
	ReceiptRoot          [32]byte
	DataAvailabilityRoot [32]byte
	SequencerId          [32]byte //nolint:revive,stylecheck
	TimestampMs          uint64
	Status               uint8
	Signers              uint8
	AvailabilityMask     uint8
	OpenChallenges       uint32
	SubmittedHeight      uint64
	FinalizedHeight      uint64
}

// GuarantorView is the ABI tuple returned by guarantor(bytes32).
type GuarantorView struct {
	GuarantorId [32]byte //nolint:revive,stylecheck
	Signer      common.Address
	Operator    common.Address
	Bond        *big.Int
	Unbonding   *big.Int
	Status      uint8
	Eligible    bool
}

type PrecompileExecutor struct {
	abi        abi.ABI
	address    common.Address
	anchor     utils.AnchorKeeper
	bankKeeper utils.BankKeeper
	evmKeeper  utils.EVMKeeper
}

func NewPrecompile(keepers utils.Keepers) (*pcommon.Precompile, error) {
	newAbi := pcommon.MustGetABI(f, "abi.json")
	p := &PrecompileExecutor{
		abi:        newAbi,
		address:    common.HexToAddress(LayerXAnchorAddress),
		anchor:     keepers.AnchorK(),
		bankKeeper: keepers.BankK(),
		evmKeeper:  keepers.EVMK(),
	}
	return pcommon.NewPrecompile(newAbi, p, p.address, PrecompileName), nil
}

func isView(method string) bool {
	switch method {
	case LatestFinalizedMethod, CheckpointMethod, FinalizedStateRootMethod, FinalizedReceiptRootMethod,
		GuarantorMethod, ThresholdMethod, StatusOfMethod:
		return true
	default:
		return false
	}
}

// CertificateWork reads the attestation count and the validity proof length
// from a certificate's length prefixes. A certificate too short to carry them
// reports the maximum attestation count.
func CertificateWork(certificate []byte) (attestations uint64, proofNodes uint64) {
	const proofLengthOffset = 2 + 4 + codec.BatchHeaderBytes
	if len(certificate) < proofLengthOffset+4 {
		return codec.MaxGuarantorAttestations, 0
	}
	proofLength := uint64(binary.BigEndian.Uint32(certificate[proofLengthOffset:]))
	countOffset := uint64(proofLengthOffset) + 4 + proofLength
	if proofLength > codec.MaxValidityProofBytes || countOffset >= uint64(len(certificate)) {
		return codec.MaxGuarantorAttestations, 0
	}
	attestations = uint64(certificate[countOffset])
	if attestations > codec.MaxGuarantorAttestations {
		attestations = codec.MaxGuarantorAttestations
	}
	return attestations, proofLength / 32
}

// Gas is the documented formula over already-measured quantities.
func Gas(view bool, inputBytes, signatures, proofNodes uint64) uint64 {
	base := WriteBaseGas
	if view {
		base = ViewBaseGas
	}
	return base + GasPerByte*inputBytes + GasPerSignature*signatures + GasPerProofNode*proofNodes
}

func (p PrecompileExecutor) RequiredGas(input []byte, method *abi.Method) uint64 {
	signatures, nodes := uint64(0), uint64(0)
	switch method.Name {
	case SubmitCheckpointMethod:
		signatures = 1 + codec.MaxGuarantorAttestations
		if args, err := method.Inputs.Unpack(input); err == nil && len(args) == 3 {
			if certificate, ok := args[2].([]byte); ok {
				attestations, proofNodes := CertificateWork(certificate)
				signatures, nodes = 1+attestations, proofNodes
			}
		}
	case SubmitAvailabilityMethod:
		signatures = 1
	case SubmitEquivocationMethod:
		signatures = 2
	}
	return Gas(isView(method.Name), uint64(len(input)), signatures, nodes)
}

func (p PrecompileExecutor) Execute(ctx sdk.Context, method *abi.Method, caller common.Address, _ common.Address, args []interface{}, value *big.Int, readOnly bool, evm *vm.EVM, hooks *tracing.Hooks) (ret []byte, err error) {
	defer func() {
		if recovered := recover(); recovered != nil {
			ret = nil
			err = fmt.Errorf("layerxanchor: %v", recovered)
		}
	}()
	if ctx.EVMPrecompileCalledFromDelegateCall() {
		return nil, errors.New("cannot delegatecall layerxAnchor")
	}
	if p.anchor == nil {
		return nil, errors.New("layerxanchor: anchor module is not wired")
	}
	if !isView(method.Name) && readOnly {
		return nil, errors.New("cannot change layerxAnchor state from staticcall")
	}
	switch method.Name {
	case RegisterGuarantorMethod, IncreaseBondMethod, OpenChallengeMethod:
	default:
		if err := pcommon.ValidateNonPayable(value); err != nil {
			return nil, err
		}
	}
	switch method.Name {
	case SubmitCheckpointMethod:
		return p.submitCheckpoint(ctx, method, caller, args, evm)
	case SubmitAvailabilityMethod:
		return p.submitAvailability(ctx, method, args, evm)
	case FinalizeMethod:
		checkpoint, err := p.anchor.Finalize(ctx, args[0].(uint64))
		if err != nil {
			return nil, err
		}
		if err := p.logFinalized(evm, checkpoint); err != nil {
			return nil, err
		}
		return method.Outputs.Pack(true)
	case RegisterGuarantorMethod:
		return p.registerGuarantor(ctx, method, caller, args, value, evm, hooks)
	case IncreaseBondMethod:
		return p.increaseBond(ctx, method, caller, args, value, evm, hooks)
	case BeginUnbondMethod:
		return p.beginUnbond(ctx, method, caller, args, evm)
	case CompleteUnbondMethod:
		return p.completeUnbond(ctx, method, caller, args, evm)
	case SubmitEquivocationMethod:
		return p.submitEquivocation(ctx, method, caller, args, evm)
	case OpenChallengeMethod:
		return p.openChallenge(ctx, method, caller, args, value, evm, hooks)
	case ResolveChallengeMethod:
		return p.resolveChallenge(ctx, method, caller, args, evm)
	case ActivateGuarantorMethod:
		id := args[0].([32]byte)
		if err := p.anchor.ActivateGuarantor(ctx, p.authorityAccount(ctx, caller), id); err != nil {
			return nil, err
		}
		if err := p.log(evm, "GuarantorActivated", []common.Hash{id}); err != nil {
			return nil, err
		}
		return method.Outputs.Pack(true)
	case SetSequencerAuthorizationMethod:
		authorization := anchortypes.SequencerAuthorization{SequencerID: args[0].([32]byte), PublicKey: args[1].([32]byte),
			FirstBatchNumber: args[2].(uint64), LastBatchNumber: args[3].(uint64)}
		if err := p.anchor.SetSequencerAuthorization(ctx, p.authorityAccount(ctx, caller), authorization); err != nil {
			return nil, err
		}
		if err := p.log(evm, "SequencerAuthorized", []common.Hash{common.Hash(authorization.SequencerID)}, [32]byte(authorization.PublicKey),
			authorization.FirstBatchNumber, authorization.LastBatchNumber); err != nil {
			return nil, err
		}
		return method.Outputs.Pack(true)
	case LatestFinalizedMethod:
		batch, ok := p.anchor.LatestFinalizedBatch(ctx)
		return method.Outputs.Pack(batch, ok)
	case CheckpointMethod:
		checkpoint, _ := p.anchor.GetCheckpoint(ctx, args[0].(uint64))
		return method.Outputs.Pack(checkpointView(checkpoint))
	case FinalizedStateRootMethod:
		root, ok := p.anchor.FinalizedStateRoot(ctx, args[0].(uint64))
		return method.Outputs.Pack(root, ok)
	case FinalizedReceiptRootMethod:
		root, ok := p.anchor.FinalizedReceiptRoot(ctx, args[0].(uint64))
		return method.Outputs.Pack(root, ok)
	case GuarantorMethod:
		return p.guarantor(ctx, method, args)
	case ThresholdMethod:
		return method.Outputs.Pack(p.anchor.GetParams(ctx).Threshold)
	case StatusOfMethod:
		return method.Outputs.Pack(p.anchor.StatusOf(ctx, args[0].(uint64)))
	}
	return nil, fmt.Errorf("layerxanchor: unknown method %s", method.Name)
}

// account is the Cosmos account of an EVM caller: its associated address, or
// the address cast when it has none.
func (p PrecompileExecutor) account(ctx sdk.Context, caller common.Address) sdk.AccAddress {
	return p.evmKeeper.GetPaxAddressOrDefault(ctx, caller)
}

// authorityAccount is the account an authority call is made as. Genesis names
// the authority before its key has signed anything, as the cast of the key's
// EVM address; the key's first transaction then associates the EVM address with
// its public-key account. Only that key can call from the EVM address, so the
// cast stays its account for the authority check after association.
func (p PrecompileExecutor) authorityAccount(ctx sdk.Context, caller common.Address) sdk.AccAddress {
	if cast := sdk.AccAddress(caller[:]); p.anchor.GetParams(ctx).Authority == cast.String() {
		return cast
	}
	return p.account(ctx, caller)
}

// bondHolder is the account that owns a bond or a challenge bond. It must be
// associated: an operator recorded under a cast address would lose control of
// the bond when the key later associates.
func (p PrecompileExecutor) bondHolder(ctx sdk.Context, caller common.Address) (sdk.AccAddress, error) {
	account, associated := p.evmKeeper.GetPaxAddress(ctx, caller)
	if !associated {
		return nil, fmt.Errorf("layerxanchor: address %s is not associated", caller.Hex())
	}
	return account, nil
}

// payment turns the call value into bond coins owned by payer again, so the
// keeper can escrow them from payer into the module account.
func (p PrecompileExecutor) payment(ctx sdk.Context, payer sdk.AccAddress, value *big.Int, evm *vm.EVM, hooks *tracing.Hooks) (sdk.Int, error) {
	if value == nil || value.Sign() == 0 {
		return sdk.ZeroInt(), nil
	}
	coin, err := pcommon.HandlePaymentUhpx(ctx, p.evmKeeper.GetPaxAddressOrDefault(ctx, p.address), payer, value,
		p.bankKeeper, p.evmKeeper, hooks, evm.GetDepth())
	if err != nil {
		return sdk.ZeroInt(), err
	}
	if denom := p.anchor.GetParams(ctx).BondDenom; coin.Denom != denom {
		return sdk.ZeroInt(), fmt.Errorf("layerxanchor: bonds are held in %s, the call value is %s", denom, coin.Denom)
	}
	return coin.Amount, nil
}

func (p PrecompileExecutor) log(evm *vm.EVM, name string, indexed []common.Hash, data ...interface{}) error {
	event, ok := p.abi.Events[name]
	if !ok {
		return fmt.Errorf("layerxanchor: no event %s", name)
	}
	packed, err := event.Inputs.NonIndexed().Pack(data...)
	if err != nil {
		return err
	}
	return pcommon.EmitEVMLog(evm, p.address, append([]common.Hash{event.ID}, indexed...), packed)
}

func topicUint64(value uint64) common.Hash { return common.BigToHash(new(big.Int).SetUint64(value)) }

func (p PrecompileExecutor) logFinalized(evm *vm.EVM, checkpoint anchortypes.Checkpoint) error {
	return p.log(evm, "CheckpointFinalized", []common.Hash{topicUint64(checkpoint.BatchNumber), common.Hash(checkpoint.CheckpointID)},
		[32]byte(checkpoint.StateRoot), [32]byte(checkpoint.ReceiptRoot))
}

func (p PrecompileExecutor) logSlashed(ctx sdk.Context, evm *vm.EVM, record anchortypes.SlashRecord) error {
	reporter := common.Address{}
	if account, err := sdk.AccAddressFromBech32(record.Reporter); err == nil {
		reporter = p.evmKeeper.GetEVMAddressOrDefault(ctx, account)
	}
	return p.log(evm, "GuarantorSlashed", []common.Hash{common.Hash(record.GuarantorID)}, record.Reason, record.BatchNumber,
		record.Amount.BigInt(), reporter, record.ReporterReward.BigInt())
}

func checkpointView(c anchortypes.Checkpoint) CheckpointView {
	return CheckpointView{BatchNumber: c.BatchNumber, CheckpointId: c.CheckpointID, HeaderDigest: c.HeaderDigest, Epoch: c.Epoch,
		FirstSequence: c.FirstSequence, LastSequence: c.LastSequence, PreviousStateRoot: c.PreviousStateRoot,
		StateRoot: c.StateRoot, ReceiptRoot: c.ReceiptRoot, DataAvailabilityRoot: c.DataAvailabilityRoot,
		SequencerId: c.SequencerID, TimestampMs: c.TimestampMs, Status: c.Status, Signers: uint8(len(c.Guarantors)), //nolint:gosec
		AvailabilityMask: c.AvailabilityMask, OpenChallenges: c.OpenChallenges,
		SubmittedHeight: uint64(c.SubmittedHeight), FinalizedHeight: uint64(c.FinalizedHeight)} //nolint:gosec
}

func (p PrecompileExecutor) submitCheckpoint(ctx sdk.Context, method *abi.Method, caller common.Address, args []interface{}, evm *vm.EVM) ([]byte, error) {
	header, signature, certificate := args[0].([]byte), args[1].([]byte), args[2].([]byte)
	if len(signature) != 64 {
		return nil, errors.New("layerxanchor: header signature must be 64 bytes")
	}
	var headerSignature [64]byte
	copy(headerSignature[:], signature)
	checkpoint, err := p.anchor.SubmitCheckpoint(ctx, p.account(ctx, caller), header, headerSignature, certificate)
	if err != nil {
		return nil, err
	}
	topics := []common.Hash{topicUint64(checkpoint.BatchNumber), common.Hash(checkpoint.CheckpointID)}
	if err := p.log(evm, "CheckpointSubmitted", topics, [32]byte(checkpoint.StateRoot), [32]byte(checkpoint.ReceiptRoot),
		uint8(len(checkpoint.Guarantors))); err != nil { //nolint:gosec
		return nil, err
	}
	if checkpoint.Status == anchortypes.CheckpointFinal {
		if err := p.logFinalized(evm, checkpoint); err != nil {
			return nil, err
		}
	}
	return method.Outputs.Pack([32]byte(checkpoint.CheckpointID), checkpoint.Status)
}

func (p PrecompileExecutor) submitAvailability(ctx sdk.Context, method *abi.Method, args []interface{}, evm *vm.EVM) ([]byte, error) {
	checkpoint, record, err := p.anchor.SubmitAvailabilityAttestation(ctx, args[0].([]byte))
	if err != nil {
		return nil, err
	}
	if err := p.log(evm, "AvailabilityAttested", []common.Hash{topicUint64(record.BatchNumber), common.Hash(record.GuarantorID)},
		record.ClassMask, checkpoint.AvailabilityMask); err != nil {
		return nil, err
	}
	return method.Outputs.Pack(checkpoint.AvailabilityMask)
}

func (p PrecompileExecutor) registerGuarantor(ctx sdk.Context, method *abi.Method, caller common.Address, args []interface{}, value *big.Int, evm *vm.EVM, hooks *tracing.Hooks) ([]byte, error) {
	operator, err := p.bondHolder(ctx, caller)
	if err != nil {
		return nil, err
	}
	id, signer := args[0].([32]byte), args[1].(common.Address)
	amount, err := p.payment(ctx, operator, value, evm, hooks)
	if err != nil {
		return nil, err
	}
	guarantor, err := p.anchor.RegisterGuarantor(ctx, operator, id, signer, amount)
	if err != nil {
		return nil, err
	}
	if err := p.log(evm, "GuarantorRegistered", []common.Hash{id, common.BytesToHash(signer[:])}, caller,
		guarantor.Bond.BigInt(), guarantor.Status); err != nil {
		return nil, err
	}
	return method.Outputs.Pack(true)
}

func (p PrecompileExecutor) increaseBond(ctx sdk.Context, method *abi.Method, caller common.Address, args []interface{}, value *big.Int, evm *vm.EVM, hooks *tracing.Hooks) ([]byte, error) {
	operator, err := p.bondHolder(ctx, caller)
	if err != nil {
		return nil, err
	}
	id := args[0].([32]byte)
	amount, err := p.payment(ctx, operator, value, evm, hooks)
	if err != nil {
		return nil, err
	}
	guarantor, err := p.anchor.IncreaseBond(ctx, operator, id, amount)
	if err != nil {
		return nil, err
	}
	if err := p.log(evm, "BondIncreased", []common.Hash{id}, amount.BigInt(), guarantor.Bond.BigInt()); err != nil {
		return nil, err
	}
	return method.Outputs.Pack(true)
}

func (p PrecompileExecutor) beginUnbond(ctx sdk.Context, method *abi.Method, caller common.Address, args []interface{}, evm *vm.EVM) ([]byte, error) {
	operator, err := p.bondHolder(ctx, caller)
	if err != nil {
		return nil, err
	}
	id := args[0].([32]byte)
	entry, err := p.anchor.BeginUnbond(ctx, operator, id, sdk.NewIntFromBigInt(args[1].(*big.Int)))
	if err != nil {
		return nil, err
	}
	completion := uint64(entry.CompletionTime) //nolint:gosec
	if err := p.log(evm, "UnbondBegun", []common.Hash{id}, entry.Amount.BigInt(), completion); err != nil {
		return nil, err
	}
	return method.Outputs.Pack(completion)
}

func (p PrecompileExecutor) completeUnbond(ctx sdk.Context, method *abi.Method, caller common.Address, args []interface{}, evm *vm.EVM) ([]byte, error) {
	operator, err := p.bondHolder(ctx, caller)
	if err != nil {
		return nil, err
	}
	id := args[0].([32]byte)
	amount, err := p.anchor.CompleteUnbond(ctx, operator, id)
	if err != nil {
		return nil, err
	}
	if err := p.log(evm, "UnbondCompleted", []common.Hash{id}, amount.BigInt()); err != nil {
		return nil, err
	}
	return method.Outputs.Pack(amount.BigInt())
}

func (p PrecompileExecutor) submitEquivocation(ctx sdk.Context, method *abi.Method, caller common.Address, args []interface{}, evm *vm.EVM) ([]byte, error) {
	record, err := p.anchor.SubmitEquivocation(ctx, p.account(ctx, caller), args[0].([]byte), args[1].([]byte))
	if err != nil {
		return nil, err
	}
	if err := p.logSlashed(ctx, evm, record); err != nil {
		return nil, err
	}
	return method.Outputs.Pack(record.Amount.BigInt())
}

func (p PrecompileExecutor) openChallenge(ctx sdk.Context, method *abi.Method, caller common.Address, args []interface{}, value *big.Int, evm *vm.EVM, hooks *tracing.Hooks) ([]byte, error) {
	challenger, err := p.bondHolder(ctx, caller)
	if err != nil {
		return nil, err
	}
	bond, err := p.payment(ctx, challenger, value, evm, hooks)
	if err != nil {
		return nil, err
	}
	challenge, err := p.anchor.OpenChallenge(ctx, challenger, args[0].(uint64), args[1].(uint8), args[2].([32]byte), bond)
	if err != nil {
		return nil, err
	}
	if err := p.log(evm, "ChallengeOpened", []common.Hash{topicUint64(challenge.ID), topicUint64(challenge.BatchNumber)},
		challenge.Kind, [32]byte(challenge.EvidenceHash), caller); err != nil {
		return nil, err
	}
	return method.Outputs.Pack(challenge.ID)
}

func (p PrecompileExecutor) resolveChallenge(ctx sdk.Context, method *abi.Method, caller common.Address, args []interface{}, evm *vm.EVM) ([]byte, error) {
	upheld := args[1].(bool)
	challenge, slashed, err := p.anchor.ResolveChallenge(ctx, p.authorityAccount(ctx, caller), args[0].(uint64), upheld)
	if err != nil {
		return nil, err
	}
	for _, record := range slashed {
		if err := p.logSlashed(ctx, evm, record); err != nil {
			return nil, err
		}
	}
	if err := p.log(evm, "ChallengeResolved", []common.Hash{topicUint64(challenge.ID), topicUint64(challenge.BatchNumber)}, upheld); err != nil {
		return nil, err
	}
	return method.Outputs.Pack(true)
}

func (p PrecompileExecutor) guarantor(ctx sdk.Context, method *abi.Method, args []interface{}) ([]byte, error) {
	id := args[0].([32]byte)
	view := GuarantorView{GuarantorId: id, Bond: new(big.Int), Unbonding: new(big.Int)}
	if guarantor, ok := p.anchor.GetGuarantor(ctx, id); ok {
		params := p.anchor.GetParams(ctx)
		view.Signer = common.Address(guarantor.Signer)
		if operator, err := sdk.AccAddressFromBech32(guarantor.Operator); err == nil {
			view.Operator = p.evmKeeper.GetEVMAddressOrDefault(ctx, operator)
		}
		view.Bond = guarantor.Bond.BigInt()
		view.Status = guarantor.Status
		view.Eligible = guarantor.Status == anchortypes.GuarantorActive && guarantor.Bond.GTE(params.MinBond)
		unbonding := sdk.ZeroInt()
		for _, entry := range p.anchor.GetUnbondings(ctx) {
			if entry.GuarantorID == guarantor.ID {
				unbonding = unbonding.Add(entry.Amount)
			}
		}
		view.Unbonding = unbonding.BigInt()
	}
	return method.Outputs.Pack(view)
}
