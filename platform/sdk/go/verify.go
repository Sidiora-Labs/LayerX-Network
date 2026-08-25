package layerx

import (
	"bytes"
	"context"
	"crypto/ed25519"
	"crypto/sha256"
	"encoding/binary"
)

var (
	merkleLeafDomain            = []byte("LXP/v1/merkle-leaf\x00")
	merkleInternalDomain        = []byte("LXP/v1/merkle-internal\x00")
	batchHeaderDomain           = []byte("LXP/v1/batch-header\x00")
	receiptDomain               = []byte("LXP/v1/receipt\x00")
	checkpointDomain            = []byte("LXP/v1/checkpoint-certificate\x00")
	guarantorAttestationDomain = []byte("LXP/v1/guarantor-attestation\x00")
)

const (
	maximumMessageBytes    = 1_048_576
	maximumEffects         = 512
	maximumEffectBody      = 256
	batchHeaderBytes       = 354
	allAvailabilityClasses = 0x1f
)

type ReceiptCheck string

const (
	ReceiptCheckDecode             ReceiptCheck = "decode"
	ReceiptCheckCanonicalEncoding  ReceiptCheck = "canonical-encoding"
	ReceiptCheckReceiptShape       ReceiptCheck = "receipt-shape"
	ReceiptCheckMissingSignature   ReceiptCheck = "missing-signature"
	ReceiptCheckProtocolVersion    ReceiptCheck = "protocol-version"
	ReceiptCheckResultCode         ReceiptCheck = "result-code"
	ReceiptCheckOperation          ReceiptCheck = "operation"
	ReceiptCheckActivityID         ReceiptCheck = "activity-id"
	ReceiptCheckBatchID            ReceiptCheck = "batch-id"
	ReceiptCheckAsset              ReceiptCheck = "asset"
	ReceiptCheckPreviousStateRoot  ReceiptCheck = "previous-state-root"
	ReceiptCheckResultingStateRoot ReceiptCheck = "resulting-state-root"
	ReceiptCheckDebitBalance       ReceiptCheck = "debit-balance"
	ReceiptCheckCreditBalance      ReceiptCheck = "credit-balance"
	ReceiptCheckSequencerSignature ReceiptCheck = "sequencer-signature"
)

type VerificationError struct{ Check ReceiptCheck }

func (*VerificationError) Error() string { return "Local verification failed." }
func (*VerificationError) Unwrap() error { return ErrVerification }

func receiptFailure(check ReceiptCheck) error { return &VerificationError{Check: check} }
func verificationFailure() error              { return newSDKError(ErrorVerificationFailure, RetryNever) }

type AuthorizedBatch struct {
	BatchID            [32]byte
	Asset              [32]byte
	PreviousStateRoot  [32]byte
	ResultingStateRoot [32]byte
	SequencerPublicKey [32]byte
}

type ProtocolReceiptFacts struct {
	ResultCode int32
	Asset      [32]byte
	Amount     Uint128
	FeeCharged Uint128
}

type ReceiptEffect struct {
	ModuleID        uint16
	Ordinal         uint16
	EventType       uint16
	Kind            uint8
	Monetary        bool
	TransferSetRoot [32]byte
	Body            []byte
}

type ProtocolReceipt struct {
	ProtocolVersion    uint16
	ActivityID         [32]byte
	GlobalSequence     uint64
	PreviousStateRoot  [32]byte
	ResultingStateRoot [32]byte
	ActivityRoot       [32]byte
	ResultCode         int32
	Effects            []ReceiptEffect
	FeeCharged         Uint128
	BatchID            [32]byte
	ModuleID           uint16
	ModuleVersion      uint32
	ParameterVersion   uint32
	Operation          uint8
	Asset              [32]byte
	Amount             Uint128
	From               [32]byte
	FromBalanceBefore  Uint128
	FromBalanceAfter   Uint128
	FromSequence       uint64
	To                 [32]byte
	ToBalanceBefore    Uint128
	ToBalanceAfter     Uint128
	TransferSetRoot    [32]byte
	AuthorizationHash  [32]byte
	ContextHash        [32]byte
	Timestamp          uint64
	SequencerSignature [64]byte
}

type VerifiedReceipt struct {
	Level          string
	Receipt        ProtocolReceipt
	CanonicalBytes []byte
	ReceiptDigest  [32]byte
	Facts          ProtocolReceiptFacts
}

type decodedReceipt struct {
	protocolVersion    uint16
	activityID         [32]byte
	previousStateRoot  [32]byte
	resultingStateRoot [32]byte
	resultCode         int32
	feeCharged         Uint128
	batchID            [32]byte
	operation          uint8
	asset              [32]byte
	amount             Uint128
	debitBefore        Uint128
	debitAfter         Uint128
	creditBefore       Uint128
	creditAfter        Uint128
	signature          [64]byte
	unsigned           []byte
	protocol           ProtocolReceipt
}

func VerifyReceiptOutcome(canonicalReceipt []byte, authorized AuthorizedBatch) (VerifiedReceipt, error) {
	receipt, err := decodeProtocolReceipt(canonicalReceipt)
	if err != nil {
		return VerifiedReceipt{}, err
	}
	if receipt.protocolVersion != 1 {
		return VerifiedReceipt{}, receiptFailure(ReceiptCheckProtocolVersion)
	}
	if receipt.operation == 0 {
		return VerifiedReceipt{}, receiptFailure(ReceiptCheckOperation)
	}
	if zero32(receipt.activityID) {
		return VerifiedReceipt{}, receiptFailure(ReceiptCheckActivityID)
	}
	if receipt.batchID != authorized.BatchID {
		return VerifiedReceipt{}, receiptFailure(ReceiptCheckBatchID)
	}
	if receipt.asset != authorized.Asset || zero32(receipt.asset) {
		return VerifiedReceipt{}, receiptFailure(ReceiptCheckAsset)
	}
	if receipt.previousStateRoot != authorized.PreviousStateRoot {
		return VerifiedReceipt{}, receiptFailure(ReceiptCheckPreviousStateRoot)
	}
	if receipt.resultingStateRoot != authorized.ResultingStateRoot {
		return VerifiedReceipt{}, receiptFailure(ReceiptCheckResultingStateRoot)
	}
	if receipt.resultCode == 0 {
		debitAfter, ok := receipt.debitBefore.Sub(receipt.amount)
		if !ok || !debitAfter.Equal(receipt.debitAfter) {
			return VerifiedReceipt{}, receiptFailure(ReceiptCheckDebitBalance)
		}
		creditAfter, ok := receipt.creditBefore.Add(receipt.amount)
		if !ok || !creditAfter.Equal(receipt.creditAfter) {
			return VerifiedReceipt{}, receiptFailure(ReceiptCheckCreditBalance)
		}
	}
	digest := domainDigest(receiptDomain, receipt.unsigned)
	if !ed25519.Verify(authorized.SequencerPublicKey[:], digest[:], receipt.signature[:]) {
		return VerifiedReceipt{}, receiptFailure(ReceiptCheckSequencerSignature)
	}
	return VerifiedReceipt{
		Level:          "sequencer-signed",
		Receipt:        receipt.protocol,
		CanonicalBytes: append([]byte(nil), canonicalReceipt...),
		ReceiptDigest:  digest,
		Facts: ProtocolReceiptFacts{
			ResultCode: receipt.resultCode,
			Asset:      receipt.asset,
			Amount:     receipt.amount,
			FeeCharged: receipt.feeCharged,
		},
	}, nil
}

func VerifyReceipt(canonicalReceipt []byte, authorized AuthorizedBatch) (VerifiedReceipt, error) {
	verified, err := VerifyReceiptOutcome(canonicalReceipt, authorized)
	if err != nil {
		return VerifiedReceipt{}, err
	}
	if verified.Facts.ResultCode != 0 {
		return VerifiedReceipt{}, receiptFailure(ReceiptCheckResultCode)
	}
	return verified, nil
}

func decodeProtocolReceipt(value []byte) (decodedReceipt, error) {
	if len(value) > maximumMessageBytes || len(value) < 4 || !bytes.Equal(value[:4], []byte{0, 1, 0x52, 1}) {
		return decodedReceipt{}, receiptFailure(ReceiptCheckReceiptShape)
	}
	decoder := wireDecoder{value: value}
	if decoder.u16() != 1 || decoder.u16() != 0x5201 {
		return decodedReceipt{}, receiptFailure(ReceiptCheckDecode)
	}
	receipt := decodedReceipt{protocolVersion: decoder.u16()}
	receipt.protocol.ProtocolVersion = receipt.protocolVersion
	receipt.activityID = decoder.array32()
	receipt.protocol.ActivityID = receipt.activityID
	receipt.protocol.GlobalSequence = decoder.u64()
	receipt.previousStateRoot = decoder.array32()
	receipt.protocol.PreviousStateRoot = receipt.previousStateRoot
	receipt.resultingStateRoot = decoder.array32()
	receipt.protocol.ResultingStateRoot = receipt.resultingStateRoot
	receipt.protocol.ActivityRoot = decoder.array32()
	receipt.resultCode = decoder.i32()
	receipt.protocol.ResultCode = receipt.resultCode
	effectCount := decoder.u32()
	if decoder.failed || effectCount > maximumEffects {
		return decodedReceipt{}, receiptFailure(ReceiptCheckDecode)
	}
	for index := uint32(0); index < effectCount; index++ {
		effect := ReceiptEffect{
			ModuleID:  decoder.u16(),
			Ordinal:   decoder.u16(),
			EventType: decoder.u16(),
		}
		kind := decoder.u8()
		monetary := decoder.u8()
		if kind == 0 || kind > 3 || monetary > 1 || monetary == 1 && kind != 2 {
			return decodedReceipt{}, receiptFailure(ReceiptCheckDecode)
		}
		effect.Kind = kind
		effect.Monetary = monetary == 1
		effect.TransferSetRoot = decoder.array32()
		effect.Body = append([]byte(nil), decoder.bounded(maximumEffectBody)...)
		receipt.protocol.Effects = append(receipt.protocol.Effects, effect)
	}
	receipt.feeCharged = decoder.u128()
	receipt.protocol.FeeCharged = receipt.feeCharged
	receipt.batchID = decoder.array32()
	receipt.protocol.BatchID = receipt.batchID
	receipt.protocol.ModuleID = decoder.u16()
	receipt.protocol.ModuleVersion = decoder.u32()
	receipt.protocol.ParameterVersion = decoder.u32()
	receipt.operation = decoder.u8()
	receipt.protocol.Operation = receipt.operation
	receipt.asset = decoder.array32()
	receipt.protocol.Asset = receipt.asset
	receipt.amount = decoder.u128()
	receipt.protocol.Amount = receipt.amount
	receipt.protocol.From = decoder.array32()
	receipt.debitBefore = decoder.u128()
	receipt.protocol.FromBalanceBefore = receipt.debitBefore
	receipt.debitAfter = decoder.u128()
	receipt.protocol.FromBalanceAfter = receipt.debitAfter
	receipt.protocol.FromSequence = decoder.u64()
	receipt.protocol.To = decoder.array32()
	receipt.creditBefore = decoder.u128()
	receipt.protocol.ToBalanceBefore = receipt.creditBefore
	receipt.creditAfter = decoder.u128()
	receipt.protocol.ToBalanceAfter = receipt.creditAfter
	receipt.protocol.TransferSetRoot = decoder.array32()
	receipt.protocol.AuthorizationHash = decoder.array32()
	receipt.protocol.ContextHash = decoder.array32()
	receipt.protocol.Timestamp = decoder.u64()
	signatureMarker := decoder.offset
	present := decoder.u8()
	if decoder.failed {
		return decodedReceipt{}, receiptFailure(ReceiptCheckDecode)
	}
	if present == 0 {
		return decodedReceipt{}, receiptFailure(ReceiptCheckMissingSignature)
	}
	if present != 1 {
		return decodedReceipt{}, receiptFailure(ReceiptCheckCanonicalEncoding)
	}
	signature := decoder.bounded(64)
	if decoder.failed || len(signature) != 64 || decoder.offset != len(value) {
		return decodedReceipt{}, receiptFailure(ReceiptCheckCanonicalEncoding)
	}
	copy(receipt.signature[:], signature)
	receipt.protocol.SequencerSignature = receipt.signature
	receipt.unsigned = make([]byte, signatureMarker+1)
	copy(receipt.unsigned, value[:signatureMarker])
	receipt.unsigned[signatureMarker] = 0
	return receipt, nil
}

type MerkleProof struct {
	LeafIndex uint32
	LeafCount uint32
	Siblings  [][32]byte
}

func VerifyMerkleInclusion(canonicalLeaf []byte, proof MerkleProof, expectedRoot [32]byte) error {
	if proof.LeafCount == 0 || proof.LeafIndex >= proof.LeafCount || len(proof.Siblings) > 32 || len(proof.Siblings) != proofDepth(proof.LeafCount) {
		return verificationFailure()
	}
	current := domainDigest(merkleLeafDomain, canonicalLeaf)
	index := proof.LeafIndex
	count := proof.LeafCount
	for _, sibling := range proof.Siblings {
		if index^1 >= count && sibling != current {
			return verificationFailure()
		}
		if index%2 == 0 {
			current = domainDigest(merkleInternalDomain, current[:], sibling[:])
		} else {
			current = domainDigest(merkleInternalDomain, sibling[:], current[:])
		}
		index /= 2
		count = (count + 1) / 2
	}
	if current != expectedRoot {
		return verificationFailure()
	}
	return nil
}

func proofDepth(count uint32) int {
	depth := 0
	for count > 1 {
		count = (count + 1) / 2
		depth++
	}
	return depth
}

type BatchHeader struct {
	ProtocolVersion      uint16
	NetworkID            uint32
	Epoch                uint64
	BatchNumber          uint64
	FirstSequence        uint64
	LastSequence         uint64
	PreviousStateRoot    [32]byte
	ResultingStateRoot   [32]byte
	ActivityMerkleRoot   [32]byte
	ReceiptMerkleRoot    [32]byte
	EventMerkleRoot      [32]byte
	DataAvailabilityRoot [32]byte
	OracleRoot           [32]byte
	TimestampMillis      uint64
	SequencerID          [32]byte
}

func DecodeBatchHeader(canonicalHeader []byte) (BatchHeader, error) {
	if len(canonicalHeader) != batchHeaderBytes {
		return BatchHeader{}, verificationFailure()
	}
	decoder := wireDecoder{value: canonicalHeader}
	if decoder.u16() != 1 || decoder.u16() != 0x1701 || decoder.u8() != 15 {
		return BatchHeader{}, verificationFailure()
	}
	field := func(expected uint8) bool { return decoder.u8() == expected }
	var header BatchHeader
	if !field(1) {
		return BatchHeader{}, verificationFailure()
	}
	header.ProtocolVersion = decoder.u16()
	if !field(2) {
		return BatchHeader{}, verificationFailure()
	}
	header.NetworkID = decoder.u32()
	if !field(3) {
		return BatchHeader{}, verificationFailure()
	}
	header.Epoch = decoder.u64()
	if !field(4) {
		return BatchHeader{}, verificationFailure()
	}
	header.BatchNumber = decoder.u64()
	if !field(5) {
		return BatchHeader{}, verificationFailure()
	}
	header.FirstSequence = decoder.u64()
	if !field(6) {
		return BatchHeader{}, verificationFailure()
	}
	header.LastSequence = decoder.u64()
	if !field(7) {
		return BatchHeader{}, verificationFailure()
	}
	header.PreviousStateRoot = decoder.array32()
	if !field(8) {
		return BatchHeader{}, verificationFailure()
	}
	header.ResultingStateRoot = decoder.array32()
	if !field(9) {
		return BatchHeader{}, verificationFailure()
	}
	header.ActivityMerkleRoot = decoder.array32()
	if !field(10) {
		return BatchHeader{}, verificationFailure()
	}
	header.ReceiptMerkleRoot = decoder.array32()
	if !field(11) {
		return BatchHeader{}, verificationFailure()
	}
	header.EventMerkleRoot = decoder.array32()
	if !field(12) {
		return BatchHeader{}, verificationFailure()
	}
	header.DataAvailabilityRoot = decoder.array32()
	if !field(13) {
		return BatchHeader{}, verificationFailure()
	}
	header.OracleRoot = decoder.array32()
	if !field(14) {
		return BatchHeader{}, verificationFailure()
	}
	header.TimestampMillis = decoder.u64()
	if !field(15) {
		return BatchHeader{}, verificationFailure()
	}
	header.SequencerID = decoder.array32()
	if decoder.failed || decoder.offset != len(canonicalHeader) {
		return BatchHeader{}, verificationFailure()
	}
	return header, nil
}

type SequencerAuthorization struct {
	SequencerID      [32]byte
	PublicKey        [32]byte
	FirstBatchNumber uint64
	LastBatchNumber  uint64
}

type InclusionKind string

const (
	InclusionActivity InclusionKind = "activity"
	InclusionReceipt  InclusionKind = "receipt"
	InclusionEvent    InclusionKind = "event"
	InclusionState    InclusionKind = "state"
)

type InclusionVerification struct {
	Level        string
	Header       BatchHeader
	HeaderDigest [32]byte
	Root         [32]byte
}

func VerifyBatchInclusion(kind InclusionKind, canonicalLeaf []byte, proof MerkleProof, canonicalHeader []byte, headerSignature []byte, authorization SequencerAuthorization) (InclusionVerification, error) {
	header, err := DecodeBatchHeader(canonicalHeader)
	if err != nil {
		return InclusionVerification{}, err
	}
	if header.BatchNumber < authorization.FirstBatchNumber || header.BatchNumber > authorization.LastBatchNumber || header.SequencerID != authorization.SequencerID || len(headerSignature) != ed25519.SignatureSize {
		return InclusionVerification{}, verificationFailure()
	}
	digest := domainDigest(batchHeaderDomain, canonicalHeader)
	if !ed25519.Verify(authorization.PublicKey[:], digest[:], headerSignature) {
		return InclusionVerification{}, verificationFailure()
	}
	var root [32]byte
	level := "batch-included"
	switch kind {
	case InclusionActivity:
		root = header.ActivityMerkleRoot
	case InclusionReceipt:
		root = header.ReceiptMerkleRoot
	case InclusionEvent:
		root = header.EventMerkleRoot
	case InclusionState:
		root = header.ResultingStateRoot
		level = "state-proven"
	default:
		return InclusionVerification{}, verificationFailure()
	}
	if err := VerifyMerkleInclusion(canonicalLeaf, proof, root); err != nil {
		return InclusionVerification{}, err
	}
	return InclusionVerification{Level: level, Header: header, HeaderDigest: digest, Root: root}, nil
}

type CheckpointAttestation struct {
	ProtocolVersion       uint16
	NetworkID             uint32
	PaxeerChainID         uint64
	SettlementContract    [20]byte
	Epoch                 uint64
	CheckpointID          [32]byte
	CheckpointHash        [32]byte
	GuarantorID           [32]byte
	BatchNumber           uint64
	DataAvailabilityRoot  [32]byte
	Replayed              bool
	DataPossessed         bool
	AvailabilityClassMask uint8
	AttestedAtMillis      uint64
	Signer                [20]byte
	Signature             [64]byte
	SignatureV            uint8
}

type GuarantorKey struct {
	GuarantorID [32]byte
	PublicKey   [33]byte
	Bonded      bool
}

type CheckpointCertificate struct {
	CanonicalHeader     []byte
	ValidityProof       []byte
	Attestations        []CheckpointAttestation
	Threshold           uint32
	SettlementReference []byte
}

type CheckpointVerificationInput struct {
	Certificate                   CheckpointCertificate
	BondedSet                     []GuarantorKey
	RegisteredCheckpointID        [32]byte
	RegisteredSettlementReference []byte
	AvailabilityObtained          bool
}

type Secp256k1Verifier interface {
	VerifySecp256k1(context.Context, [33]byte, [64]byte, [32]byte) bool
}

type CheckpointVerification struct {
	Level        string
	CheckpointID [32]byte
	Achieved     uint32
	Required     uint32
	Header       BatchHeader
}

func VerifyCheckpoint(ctx context.Context, input CheckpointVerificationInput, signatures Secp256k1Verifier) (CheckpointVerification, error) {
	certificate := input.Certificate
	if ctx == nil || signatures == nil || !input.AvailabilityObtained || uint64(len(certificate.ValidityProof)) > uint64(^uint32(0)) || certificate.Threshold == 0 {
		return CheckpointVerification{}, verificationFailure()
	}
	header, err := DecodeBatchHeader(certificate.CanonicalHeader)
	if err != nil {
		return CheckpointVerification{}, err
	}
	length := make([]byte, 4)
	binary.BigEndian.PutUint32(length, uint32(len(certificate.ValidityProof)))
	checkpointID := domainDigest(checkpointDomain, certificate.CanonicalHeader, length, certificate.ValidityProof)
	if checkpointID != input.RegisteredCheckpointID {
		return CheckpointVerification{}, verificationFailure()
	}
	bonded := make(map[[32]byte]GuarantorKey, len(input.BondedSet))
	for _, member := range input.BondedSet {
		if member.Bonded {
			bonded[member.GuarantorID] = member
		}
	}
	seen := make(map[[32]byte]struct{}, len(certificate.Attestations))
	var achieved uint32
	var paxeerChainID uint64
	var settlementContract [20]byte
	for _, attestation := range certificate.Attestations {
		if err := ctx.Err(); err != nil {
			return CheckpointVerification{}, err
		}
		if _, duplicate := seen[attestation.GuarantorID]; duplicate ||
			attestation.ProtocolVersion != header.ProtocolVersion || attestation.NetworkID != header.NetworkID || attestation.Epoch != header.Epoch ||
			attestation.PaxeerChainID == 0 || attestation.SettlementContract == ([20]byte{}) ||
			(achieved > 0 && (attestation.PaxeerChainID != paxeerChainID || attestation.SettlementContract != settlementContract)) ||
			attestation.CheckpointID != checkpointID || attestation.CheckpointHash != checkpointID ||
			attestation.BatchNumber != header.BatchNumber || attestation.DataAvailabilityRoot != header.DataAvailabilityRoot ||
			!attestation.Replayed || !attestation.DataPossessed || attestation.AvailabilityClassMask != allAvailabilityClasses || attestation.AttestedAtMillis == 0 ||
			attestation.Signer == ([20]byte{}) || (attestation.SignatureV != 27 && attestation.SignatureV != 28) {
			return CheckpointVerification{}, verificationFailure()
		}
		member, ok := bonded[attestation.GuarantorID]
		if !ok {
			return CheckpointVerification{}, verificationFailure()
		}
		seen[attestation.GuarantorID] = struct{}{}
		paxeerChainID = attestation.PaxeerChainID
		settlementContract = attestation.SettlementContract
		message := make([]byte, 0, 189)
		message = appendUint16(message, attestation.ProtocolVersion)
		message = appendUint32(message, attestation.NetworkID)
		message = appendUint64(message, attestation.PaxeerChainID)
		message = append(message, attestation.SettlementContract[:]...)
		message = appendUint64(message, attestation.Epoch)
		message = append(message, attestation.CheckpointID[:]...)
		message = append(message, attestation.CheckpointHash[:]...)
		message = append(message, attestation.GuarantorID[:]...)
		message = appendUint64(message, attestation.BatchNumber)
		message = append(message, attestation.DataAvailabilityRoot[:]...)
		message = append(message, boolByte(attestation.Replayed), boolByte(attestation.DataPossessed), attestation.AvailabilityClassMask)
		message = appendUint64(message, attestation.AttestedAtMillis)
		digest := domainDigest(guarantorAttestationDomain, message)
		if !signatures.VerifySecp256k1(ctx, member.PublicKey, attestation.Signature, digest) {
			return CheckpointVerification{}, verificationFailure()
		}
		achieved++
	}
	if achieved < certificate.Threshold {
		return CheckpointVerification{}, verificationFailure()
	}
	level := "checkpoint-finalised"
	if certificate.SettlementReference != nil {
		if len(certificate.SettlementReference) == 0 || !bytes.Equal(certificate.SettlementReference, input.RegisteredSettlementReference) {
			return CheckpointVerification{}, verificationFailure()
		}
		level = "settlement-anchored"
	}
	return CheckpointVerification{Level: level, CheckpointID: checkpointID, Achieved: achieved, Required: certificate.Threshold, Header: header}, nil
}

type wireDecoder struct {
	value  []byte
	offset int
	failed bool
}

func (decoder *wireDecoder) fixed(length int) []byte {
	if decoder.failed || length < 0 || decoder.offset > len(decoder.value)-length {
		decoder.failed = true
		return nil
	}
	result := decoder.value[decoder.offset : decoder.offset+length]
	decoder.offset += length
	return result
}

func (decoder *wireDecoder) u8() uint8 {
	value := decoder.fixed(1)
	if len(value) != 1 {
		return 0
	}
	return value[0]
}

func (decoder *wireDecoder) u16() uint16 {
	value := decoder.fixed(2)
	if len(value) != 2 {
		return 0
	}
	return binary.BigEndian.Uint16(value)
}

func (decoder *wireDecoder) u32() uint32 {
	value := decoder.fixed(4)
	if len(value) != 4 {
		return 0
	}
	return binary.BigEndian.Uint32(value)
}

func (decoder *wireDecoder) i32() int32 { return int32(decoder.u32()) }

func (decoder *wireDecoder) u64() uint64 {
	value := decoder.fixed(8)
	if len(value) != 8 {
		return 0
	}
	return binary.BigEndian.Uint64(value)
}

func (decoder *wireDecoder) u128() Uint128 {
	return Uint128{high: decoder.u64(), low: decoder.u64()}
}

func (decoder *wireDecoder) bounded(maximum uint32) []byte {
	length := decoder.u32()
	if decoder.failed || length > maximum {
		decoder.failed = true
		return nil
	}
	return decoder.fixed(int(length))
}

func (decoder *wireDecoder) array32() [32]byte {
	value := decoder.bounded(32)
	var result [32]byte
	if len(value) != len(result) {
		decoder.failed = true
		return result
	}
	copy(result[:], value)
	return result
}

func domainDigest(domain []byte, values ...[]byte) [32]byte {
	hash := sha256.New()
	hash.Write(domain)
	for _, value := range values {
		hash.Write(value)
	}
	var result [32]byte
	copy(result[:], hash.Sum(nil))
	return result
}

func zero32(value [32]byte) bool { return value == [32]byte{} }

func putUint64(target []byte, value uint64) { binary.BigEndian.PutUint64(target, value) }

func appendUint16(target []byte, value uint16) []byte {
	var encoded [2]byte
	binary.BigEndian.PutUint16(encoded[:], value)
	return append(target, encoded[:]...)
}

func appendUint32(target []byte, value uint32) []byte {
	var encoded [4]byte
	binary.BigEndian.PutUint32(encoded[:], value)
	return append(target, encoded[:]...)
}

func appendUint64(target []byte, value uint64) []byte {
	var encoded [8]byte
	binary.BigEndian.PutUint64(encoded[:], value)
	return append(target, encoded[:]...)
}

func boolByte(value bool) byte {
	if value {
		return 1
	}
	return 0
}
