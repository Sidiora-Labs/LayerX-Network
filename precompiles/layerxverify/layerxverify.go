// Package layerxverify is the stateless precompile through which EVM contracts
// verify LayerX evidence: strict Ed25519 signatures over LayerX signed objects,
// sequencer-signed receipts, receipt inclusion under a sequencer-signed batch
// header, native state proofs and program discovery proofs.
//
// Every method is pure. The caller supplies its trust anchors (sequencer key,
// sequencer identity and authorised batch range, state root, program
// identifier) as calldata; the precompile reads no chain state.
//
// Gas is charged by the EVM from RequiredGas before Execute runs:
//
//	gas = BaseGas
//	    + GasPerByte      * len(calldata after the selector)
//	    + GasPerSignature * signatures(method)
//	    + GasPerProofNode * proofNodes(method, args)
//
// signatures(method) is 1 for verifyEd25519, verifyReceipt and
// verifyDiscoveryProof, 2 for verifyReceiptInclusion (receipt and batch
// header) and 0 for verifyStateProof. proofNodes is len(proof)/32 for
// verifyReceiptInclusion and len(witness)/32 for verifyStateProof: an upper
// bound on the hashed path nodes that is known before anything is decoded.
// Calldata that does not ABI-decode is charged without the proof-node term
// and reverts in Run.
package layerxverify

import (
	"embed"
	"errors"
	"fmt"
	"math/big"

	"github.com/ethereum/go-ethereum/accounts/abi"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/tracing"
	"github.com/ethereum/go-ethereum/core/vm"
	"github.com/sidiora-labs/paxeer-network/layerxproof/codec"
	"github.com/sidiora-labs/paxeer-network/layerxproof/verify"
	pcommon "github.com/sidiora-labs/paxeer-network/precompiles/common"
	"github.com/sidiora-labs/paxeer-network/precompiles/utils"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

const (
	VerifyEd25519Method          = "verifyEd25519"
	VerifyReceiptMethod          = "verifyReceipt"
	VerifyReceiptInclusionMethod = "verifyReceiptInclusion"
	VerifyStateProofMethod       = "verifyStateProof"
	VerifyDiscoveryProofMethod   = "verifyDiscoveryProof"
)

const (
	LayerXVerifyAddress = "0x0000000000000000000000000000000000001012"
	PrecompileName      = "layerxVerify"

	// BaseGas is charged for every call.
	BaseGas uint64 = 3000
	// GasPerByte covers ABI decoding, canonical decoding and SHA-256 over the
	// supplied bytes.
	GasPerByte uint64 = 16
	// GasPerSignature covers one strict Ed25519 verification including the
	// small-order checks on the key and the nonce point.
	GasPerSignature uint64 = 4000
	// GasPerProofNode covers one domain-separated SHA-256 path node.
	GasPerProofNode uint64 = 100

	// RawMessageDomain selects verification over the message bytes as given,
	// without a LayerX domain tag.
	RawMessageDomain uint8 = 255
)

//go:embed abi.json
var f embed.FS

// ReceiptFacts is the ABI tuple returned for a verified receipt.
type ReceiptFacts struct {
	ReceiptDigest      [32]byte
	ActivityId         [32]byte //nolint:revive,stylecheck
	GlobalSequence     uint64
	ResultCode         int32
	ModuleId           uint16 //nolint:revive,stylecheck
	Operation          uint8
	Asset              [32]byte
	Amount             *big.Int
	From               [32]byte
	To                 [32]byte
	PreviousStateRoot  [32]byte
	ResultingStateRoot [32]byte
	Timestamp          uint64
}

// BatchFacts is the ABI tuple returned for a verified batch header.
type BatchFacts struct {
	HeaderDigest       [32]byte
	NetworkId          uint32 //nolint:revive,stylecheck
	Epoch              uint64
	BatchNumber        uint64
	FirstSequence      uint64
	LastSequence       uint64
	PreviousStateRoot  [32]byte
	ResultingStateRoot [32]byte
	ReceiptRoot        [32]byte
	SequencerId        [32]byte //nolint:revive,stylecheck
	TimestampMs        uint64
}

// DiscoveryFacts is the ABI tuple returned for a verified discovery proof.
type DiscoveryFacts struct {
	Digest            [32]byte
	Version           uint32
	CodeHash          [32]byte
	AbiVersion        uint16
	ObservedSequence  uint64
	ObservedAt        uint64
	ValidThrough      uint64
	StateRoot         [32]byte
	HeadReceiptDigest [32]byte
}

type PrecompileExecutor struct {
	VerifyEd25519ID          []byte
	VerifyReceiptID          []byte
	VerifyReceiptInclusionID []byte
	VerifyStateProofID       []byte
	VerifyDiscoveryProofID   []byte
}

func NewPrecompile(utils.Keepers) (*pcommon.Precompile, error) {
	newAbi := pcommon.MustGetABI(f, "abi.json")
	p := &PrecompileExecutor{}
	for name, m := range newAbi.Methods {
		switch name {
		case VerifyEd25519Method:
			p.VerifyEd25519ID = m.ID
		case VerifyReceiptMethod:
			p.VerifyReceiptID = m.ID
		case VerifyReceiptInclusionMethod:
			p.VerifyReceiptInclusionID = m.ID
		case VerifyStateProofMethod:
			p.VerifyStateProofID = m.ID
		case VerifyDiscoveryProofMethod:
			p.VerifyDiscoveryProofID = m.ID
		}
	}
	return pcommon.NewPrecompile(newAbi, p, common.HexToAddress(LayerXVerifyAddress), PrecompileName), nil
}

// Signatures returns the Ed25519 verifications a method performs.
func Signatures(method string) uint64 {
	switch method {
	case VerifyEd25519Method, VerifyReceiptMethod, VerifyDiscoveryProofMethod:
		return 1
	case VerifyReceiptInclusionMethod:
		return 2
	default:
		return 0
	}
}

// proofArgument is the index of the argument carrying Merkle path nodes.
func proofArgument(method string) (int, bool) {
	switch method {
	case VerifyReceiptInclusionMethod:
		return 1, true
	case VerifyStateProofMethod:
		return 0, true
	default:
		return 0, false
	}
}

// Gas is the documented formula over already-measured quantities.
func Gas(inputBytes, signatures, proofNodes uint64) uint64 {
	return BaseGas + GasPerByte*inputBytes + GasPerSignature*signatures + GasPerProofNode*proofNodes
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
	return Gas(uint64(len(input)), Signatures(method.Name), nodes)
}

func (p PrecompileExecutor) Execute(ctx sdk.Context, method *abi.Method, _ common.Address, _ common.Address, args []interface{}, value *big.Int, _ bool, _ *vm.EVM, _ *tracing.Hooks) (ret []byte, err error) {
	defer func() {
		if recovered := recover(); recovered != nil {
			ret = nil
			err = fmt.Errorf("layerxverify: %v", recovered)
		}
	}()
	if err = pcommon.ValidateNonPayable(value); err != nil {
		return nil, err
	}
	if ctx.EVMPrecompileCalledFromDelegateCall() {
		return nil, errors.New("cannot delegatecall layerxVerify")
	}
	switch method.Name {
	case VerifyEd25519Method:
		return p.verifyEd25519(method, args)
	case VerifyReceiptMethod:
		return p.verifyReceipt(method, args)
	case VerifyReceiptInclusionMethod:
		return p.verifyReceiptInclusion(method, args)
	case VerifyStateProofMethod:
		return p.verifyStateProof(method, args)
	case VerifyDiscoveryProofMethod:
		return p.verifyDiscoveryProof(method, args)
	}
	return nil, fmt.Errorf("layerxverify: unknown method %s", method.Name)
}

func signature64(raw []byte) ([64]byte, error) {
	var out [64]byte
	if len(raw) != len(out) {
		return out, errors.New("layerxverify: signature must be 64 bytes")
	}
	copy(out[:], raw)
	return out, nil
}

// verifyEd25519 returns false for a refused signature and reverts only for
// arguments that are not a well-formed question.
func (p PrecompileExecutor) verifyEd25519(method *abi.Method, args []interface{}) ([]byte, error) {
	if err := pcommon.ValidateArgsLength(args, 4); err != nil {
		return nil, err
	}
	publicKey := args[0].([32]byte)
	domain := args[1].(uint8)
	message := args[2].([]byte)
	signature, err := signature64(args[3].([]byte))
	if err != nil {
		return nil, err
	}
	if len(message) > codec.MaxMessageBytes {
		return nil, errors.New("layerxverify: message exceeds the LayerX message bound")
	}
	if domain == RawMessageDomain {
		return method.Outputs.Pack(verify.Ed25519(publicKey, signature, message) == nil)
	}
	if _, err := codec.Domain(domain).Tag(); err != nil {
		return nil, err
	}
	return method.Outputs.Pack(verify.Ed25519Domain(publicKey, signature, codec.Domain(domain), message) == nil)
}

func receiptFacts(verified *verify.VerifiedReceipt) (ReceiptFacts, error) {
	receipt := verified.Receipt
	amount := receipt.Amount.Bytes()
	return ReceiptFacts{
		ReceiptDigest:      verified.Digest,
		ActivityId:         receipt.ActivityID,
		GlobalSequence:     receipt.GlobalSequence,
		ResultCode:         receipt.ResultCode,
		ModuleId:           receipt.ModuleID,
		Operation:          receipt.Operation,
		Asset:              receipt.Asset,
		Amount:             new(big.Int).SetBytes(amount[:]),
		From:               receipt.From,
		To:                 receipt.To,
		PreviousStateRoot:  receipt.PreviousStateRoot,
		ResultingStateRoot: receipt.ResultingStateRoot,
		Timestamp:          receipt.Timestamp,
	}, nil
}

func (p PrecompileExecutor) verifyReceipt(method *abi.Method, args []interface{}) ([]byte, error) {
	if err := pcommon.ValidateArgsLength(args, 2); err != nil {
		return nil, err
	}
	verified, err := verify.ReceiptSignature(args[0].([]byte), args[1].([32]byte))
	if err != nil {
		return nil, err
	}
	facts, err := receiptFacts(verified)
	if err != nil {
		return nil, err
	}
	return method.Outputs.Pack(facts)
}

func (p PrecompileExecutor) verifyReceiptInclusion(method *abi.Method, args []interface{}) ([]byte, error) {
	if err := pcommon.ValidateArgsLength(args, 8); err != nil {
		return nil, err
	}
	proof, err := codec.DecodeMerkleProof(args[1].([]byte))
	if err != nil {
		return nil, err
	}
	headerSignature, err := signature64(args[3].([]byte))
	if err != nil {
		return nil, err
	}
	authorization := verify.SequencerAuthorization{
		SequencerID:      args[4].([32]byte),
		PublicKey:        args[5].([32]byte),
		FirstBatchNumber: args[6].(uint64),
		LastBatchNumber:  args[7].(uint64),
	}
	if authorization.FirstBatchNumber > authorization.LastBatchNumber {
		return nil, errors.New("layerxverify: authorised batch range is reversed")
	}
	verified, header, err := verify.ReceiptInclusion(args[0].([]byte), proof, args[2].([]byte), headerSignature, authorization)
	if err != nil {
		return nil, err
	}
	facts, err := receiptFacts(verified)
	if err != nil {
		return nil, err
	}
	return method.Outputs.Pack(facts, BatchFacts{
		HeaderDigest:       header.Digest,
		NetworkId:          header.Header.NetworkID,
		Epoch:              header.Header.Epoch,
		BatchNumber:        header.Header.BatchNumber,
		FirstSequence:      header.Header.FirstSequence,
		LastSequence:       header.Header.LastSequence,
		PreviousStateRoot:  header.Header.PreviousStateRoot,
		ResultingStateRoot: header.Header.ResultingStateRoot,
		ReceiptRoot:        header.Header.ReceiptMerkleRoot,
		SequencerId:        header.Header.SequencerID,
		TimestampMs:        header.Header.TimestampMs,
	})
}

func (p PrecompileExecutor) verifyStateProof(method *abi.Method, args []interface{}) ([]byte, error) {
	if err := pcommon.ValidateArgsLength(args, 2); err != nil {
		return nil, err
	}
	witness, err := verify.StateProof(args[0].([]byte), args[1].([32]byte))
	if err != nil {
		return nil, err
	}
	return method.Outputs.Pack(witness.ModuleID, witness.Key, witness.Value)
}

func (p PrecompileExecutor) verifyDiscoveryProof(method *abi.Method, args []interface{}) ([]byte, error) {
	if err := pcommon.ValidateArgsLength(args, 5); err != nil {
		return nil, err
	}
	verified, err := verify.DiscoveryProof(args[0].([]byte), args[1].([]byte), args[2].([32]byte),
		args[3].(uint64), args[4].([32]byte))
	if err != nil {
		return nil, err
	}
	return method.Outputs.Pack(DiscoveryFacts{
		Digest:            verified.Digest,
		Version:           verified.Head.Version,
		CodeHash:          verified.Head.CodeHash,
		AbiVersion:        verified.Head.AbiVersion,
		ObservedSequence:  verified.Head.ObservedSequence,
		ObservedAt:        verified.Head.ObservedAt,
		ValidThrough:      verified.Head.ValidThrough,
		StateRoot:         verified.Head.StateRoot,
		HeadReceiptDigest: verified.HeadReceiptDigest,
	})
}
