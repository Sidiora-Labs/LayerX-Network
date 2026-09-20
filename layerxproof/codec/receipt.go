package codec

const (
	receiptTag       uint16 = 0x5201
	supplyReceiptTag uint16 = 0x5202
	// MaxEffects bounds the effect sequence of one receipt.
	MaxEffects = 512
	// MaxEffectBody bounds one effect body.
	MaxEffectBody = 256
	// signatureBlockBytes is the trailing presence byte, length prefix and signature.
	signatureBlockBytes = 1 + 4 + 64

	programOutcomeV1 uint32 = 0x50524731
	programOutcomeV2 uint32 = 0x50524732
	programOutcomeV3 uint32 = 0x50524733
	programOutcomeV4 uint32 = 0x50524734
)

// Effect is one canonical receipt effect.
type Effect struct {
	ModuleID        uint16
	Ordinal         uint16
	EventType       uint16
	Kind            uint8
	Monetary        bool
	TransferSetRoot [32]byte
	Body            []byte
}

// ProgramOutcome is the canonical Programs execution record of a receipt.
type ProgramOutcome struct {
	EncodingVersion         uint8
	TerminalKind            uint8
	ResultCode              int32
	RuntimeVersion          uint16
	AbiVersion              uint16
	FeeScheduleVersion      uint32
	MeteringScheduleVersion uint32
	CPUFuel                 uint64
	MemoryBytes             uint64
	StorageReadBytes        uint64
	StorageWriteBytes       uint64
	OutputValues            uint32
	OutputBytes             uint64
	OccupancyByteBatches    U128
	OccupancyFeeUnits       U128
	FeeSchedulePrices       [7]uint64
	OccupancyAssetID        [32]byte
	OccupancyEvidenceDigest [32]byte
	OccupancyTransferRoot   [32]byte
	FeeUnits                U128
	CallGraphRoot           [32]byte
	TerminalPayloadRoot     [32]byte
	TransferRoot            [32]byte
	AppliedLegsDigest       [32]byte
}

// Supply is the asset supply binding of a 0x5202 receipt.
type Supply struct {
	Before U128
	After  U128
}

// Receipt is a full canonical core receipt.
type Receipt struct {
	ProtocolVersion    uint16
	ActivityID         [32]byte
	GlobalSequence     uint64
	PreviousStateRoot  [32]byte
	ResultingStateRoot [32]byte
	ActivityRoot       [32]byte
	ResultCode         int32
	Effects            []Effect
	FeeCharged         U128
	BatchID            [32]byte
	ModuleID           uint16
	ModuleVersion      uint32
	ParameterVersion   uint32
	Operation          uint8
	Asset              [32]byte
	Amount             U128
	From               [32]byte
	FromBalanceBefore  U128
	FromBalanceAfter   U128
	FromSequence       uint64
	To                 [32]byte
	ToBalanceBefore    U128
	ToBalanceAfter     U128
	TransferSetRoot    [32]byte
	AuthorizationHash  [32]byte
	ContextHash        [32]byte
	Timestamp          uint64
	Supply             *Supply
	ProgramOutcome     *ProgramOutcome
	SequencerSignature *[64]byte

	canonical []byte
}

// CanonicalBytes returns the exact bytes this receipt decoded from.
func (r *Receipt) CanonicalBytes() []byte { return append([]byte(nil), r.canonical...) }

// UnsignedBytes returns the exact signing preimage: the canonical encoding
// with the sequencer signature absent.
func (r *Receipt) UnsignedBytes() []byte {
	body := r.canonical
	if r.SequencerSignature != nil {
		body = body[:len(body)-signatureBlockBytes]
	} else {
		body = body[:len(body)-1]
	}
	out := make([]byte, 0, len(body)+1)
	out = append(out, body...)
	return append(out, 0)
}

// Digest returns the receipt digest the sequencer signs.
func (r *Receipt) Digest() [32]byte { return ReceiptDigest(r.UnsignedBytes()) }

// IsProtocolReceiptShape reports the full-receipt prefix selected by the core
// decoder; any other byte string is a compact replay record or garbage.
func IsProtocolReceiptShape(bytes []byte) bool {
	return len(bytes) >= 4 && bytes[0] == 0 && bytes[1] >= 1 && bytes[1] <= 3 &&
		bytes[2] == 0x52 && bytes[3] >= 1 && bytes[3] <= 2
}

func decodeEffect(r *reader) (Effect, error) {
	var effect Effect
	var err error
	if effect.ModuleID, err = r.u16(); err != nil {
		return effect, err
	}
	if effect.Ordinal, err = r.u16(); err != nil {
		return effect, err
	}
	if effect.EventType, err = r.u16(); err != nil {
		return effect, err
	}
	if effect.Kind, err = r.u8(); err != nil {
		return effect, err
	}
	if effect.Kind > 3 || effect.Kind == 0 {
		return effect, ErrInvalidTag
	}
	monetary, err := r.u8()
	if err != nil {
		return effect, err
	}
	if monetary > 1 {
		return effect, ErrNonCanonical
	}
	effect.Monetary = monetary == 1
	if effect.Monetary && effect.Kind != 2 {
		return effect, ErrFatalInvariant
	}
	if effect.TransferSetRoot, err = r.array32(); err != nil {
		return effect, err
	}
	body, err := r.prefixed(MaxEffectBody)
	if err != nil {
		return effect, err
	}
	effect.Body = append([]byte(nil), body...)
	if effect.ModuleID == 0 {
		return effect, ErrNonCanonical
	}
	if effect.Kind == 2 && effect.TransferSetRoot == ([32]byte{}) {
		return effect, ErrFatalInvariant
	}
	return effect, nil
}

func validateProgramOutcome(o *ProgramOutcome, protocol uint16) error {
	zero := [32]byte{}
	if o.TerminalKind < 1 || o.TerminalKind > 3 || o.RuntimeVersion == 0 || o.AbiVersion == 0 ||
		o.FeeScheduleVersion == 0 || o.MeteringScheduleVersion != 1 || o.TerminalPayloadRoot == zero ||
		(o.TerminalKind == 1 && o.ResultCode != 0) ||
		(o.TerminalKind != 1 && (o.ResultCode == 0 || o.ResultCode <= -1000)) ||
		(o.TerminalKind != 1 && o.TransferRoot != zero) {
		return ErrFatalInvariant
	}
	e := o.EncodingVersion
	supported := (protocol == 1 && (e == 1 || e == 3)) ||
		((protocol == 2 || protocol == 3) && (e == 2 || e == 3)) ||
		(protocol == 3 && e == 4)
	if !supported {
		return ErrVersionUnsupported
	}
	if (e == 4) == (o.AppliedLegsDigest == zero) {
		return ErrNonCanonical
	}
	occupancyZero := o.OccupancyByteBatches.IsZero() && o.OccupancyFeeUnits.IsZero() &&
		o.OccupancyAssetID == zero && o.OccupancyEvidenceDigest == zero && o.OccupancyTransferRoot == zero
	if (e == 1 && !occupancyZero) ||
		(e >= 2 && o.TerminalKind != 1 && !occupancyZero) ||
		(e == 2 && o.TerminalKind == 1 && (o.OccupancyAssetID == zero || o.OccupancyEvidenceDigest == zero)) ||
		(e >= 3 && ((o.OccupancyAssetID == zero) != (o.OccupancyEvidenceDigest == zero))) ||
		(protocol == 1 && e >= 3 && !occupancyZero) ||
		(ProtocolVersionUsesOccupancy(protocol) && e >= 3 && o.TerminalKind == 1 &&
			(o.OccupancyAssetID == zero || o.OccupancyEvidenceDigest == zero)) {
		return ErrNonCanonical
	}
	return nil
}

func decodeProgramOutcome(r *reader, protocol uint16) (*ProgramOutcome, error) {
	o := &ProgramOutcome{MeteringScheduleVersion: 1}
	magic, err := r.u32()
	if err != nil {
		return nil, err
	}
	switch magic {
	case programOutcomeV1:
		o.EncodingVersion = 1
	case programOutcomeV2:
		o.EncodingVersion = 2
	case programOutcomeV3:
		o.EncodingVersion = 3
	case programOutcomeV4:
		o.EncodingVersion = 4
	default:
		return nil, ErrNonCanonical
	}
	if o.TerminalKind, err = r.u8(); err != nil {
		return nil, err
	}
	if o.ResultCode, err = r.i32(); err != nil {
		return nil, err
	}
	if o.RuntimeVersion, err = r.u16(); err != nil {
		return nil, err
	}
	if o.AbiVersion, err = r.u16(); err != nil {
		return nil, err
	}
	if o.FeeScheduleVersion, err = r.u32(); err != nil {
		return nil, err
	}
	if o.EncodingVersion >= 3 {
		if o.MeteringScheduleVersion, err = r.u32(); err != nil {
			return nil, err
		}
	}
	if o.CPUFuel, err = r.u64(); err != nil {
		return nil, err
	}
	if o.MemoryBytes, err = r.u64(); err != nil {
		return nil, err
	}
	if o.StorageReadBytes, err = r.u64(); err != nil {
		return nil, err
	}
	if o.StorageWriteBytes, err = r.u64(); err != nil {
		return nil, err
	}
	if o.OutputValues, err = r.u32(); err != nil {
		return nil, err
	}
	if o.OutputBytes, err = r.u64(); err != nil {
		return nil, err
	}
	if o.EncodingVersion >= 2 {
		if o.OccupancyByteBatches, err = r.u128(); err != nil {
			return nil, err
		}
		if o.OccupancyFeeUnits, err = r.u128(); err != nil {
			return nil, err
		}
		for index := range o.FeeSchedulePrices {
			if o.FeeSchedulePrices[index], err = r.u64(); err != nil {
				return nil, err
			}
		}
		if o.OccupancyAssetID, err = r.array32(); err != nil {
			return nil, err
		}
		if o.OccupancyEvidenceDigest, err = r.array32(); err != nil {
			return nil, err
		}
		if o.OccupancyTransferRoot, err = r.array32(); err != nil {
			return nil, err
		}
	}
	if o.FeeUnits, err = r.u128(); err != nil {
		return nil, err
	}
	if o.CallGraphRoot, err = r.array32(); err != nil {
		return nil, err
	}
	if o.TerminalPayloadRoot, err = r.array32(); err != nil {
		return nil, err
	}
	if o.TransferRoot, err = r.array32(); err != nil {
		return nil, err
	}
	if o.EncodingVersion == 4 {
		if o.AppliedLegsDigest, err = r.array32(); err != nil {
			return nil, err
		}
	}
	if err := validateProgramOutcome(o, protocol); err != nil {
		return nil, err
	}
	return o, nil
}

func decodeSupply(r *reader, receipt *Receipt) (*Supply, error) {
	before, err := r.u128()
	if err != nil {
		return nil, err
	}
	after, err := r.u128()
	if err != nil {
		return nil, err
	}
	var expected U128
	ok := false
	switch {
	case receipt.Operation == 1 && before.IsZero():
		expected, ok = U128{}, true
	case receipt.Operation >= 2 && receipt.Operation <= 8:
		expected, ok = before, true
	case receipt.Operation == 10 && !receipt.Amount.IsZero():
		expected, ok = before.CheckedAdd(receipt.Amount)
	case receipt.Operation == 11 && !receipt.Amount.IsZero():
		expected, ok = before.CheckedSub(receipt.Amount)
	}
	if receipt.ModuleID != 1 || receipt.ResultCode != 0 || receipt.Asset == ([32]byte{}) || !ok || expected != after {
		return nil, ErrNonCanonical
	}
	return &Supply{Before: before, After: after}, nil
}

// DecodeReceipt strictly decodes one full canonical core receipt. It applies
// every refusal of layerx-wire receipt::decode together with the structural
// refusals of lxp_receipt_decode (zero sequence, module, module version,
// timestamp, activity identifier or resulting root; unsorted effects).
func DecodeReceipt(bytes []byte) (*Receipt, error) {
	if len(bytes) > MaxMessageBytes {
		return nil, ErrLengthLimit
	}
	if !IsProtocolReceiptShape(bytes) {
		return nil, ErrReceiptShape
	}
	supplyPresent := bytes[3] == 0x02
	tag := receiptTag
	if supplyPresent {
		tag = supplyReceiptTag
	}
	r := &reader{bytes: bytes}
	receipt := &Receipt{}
	envelope, err := r.structureHeaderVersion(tag)
	if err != nil {
		return nil, err
	}
	if receipt.ProtocolVersion, err = r.u16(); err != nil {
		return nil, err
	}
	if receipt.ProtocolVersion != envelope {
		return nil, ErrVersionUnsupported
	}
	if receipt.ActivityID, err = r.array32(); err != nil {
		return nil, err
	}
	if receipt.GlobalSequence, err = r.u64(); err != nil {
		return nil, err
	}
	if receipt.PreviousStateRoot, err = r.array32(); err != nil {
		return nil, err
	}
	if receipt.ResultingStateRoot, err = r.array32(); err != nil {
		return nil, err
	}
	if receipt.ActivityRoot, err = r.array32(); err != nil {
		return nil, err
	}
	if receipt.ResultCode, err = r.i32(); err != nil {
		return nil, err
	}
	count, err := r.sequenceLength(MaxEffects)
	if err != nil {
		return nil, err
	}
	// Each effect occupies at least 48 bytes, so the count is bounded by input.
	if count > r.remaining()/48 {
		return nil, ErrTruncated
	}
	receipt.Effects = make([]Effect, 0, count)
	for index := 0; index < count; index++ {
		effect, err := decodeEffect(r)
		if err != nil {
			return nil, err
		}
		if index > 0 {
			previous := receipt.Effects[index-1]
			if previous.ModuleID > effect.ModuleID ||
				(previous.ModuleID == effect.ModuleID && previous.Ordinal >= effect.Ordinal) {
				return nil, ErrUnsortedSequence
			}
		}
		receipt.Effects = append(receipt.Effects, effect)
	}
	if receipt.FeeCharged, err = r.u128(); err != nil {
		return nil, err
	}
	if receipt.BatchID, err = r.array32(); err != nil {
		return nil, err
	}
	if receipt.ModuleID, err = r.u16(); err != nil {
		return nil, err
	}
	if receipt.ModuleVersion, err = r.u32(); err != nil {
		return nil, err
	}
	if receipt.ParameterVersion, err = r.u32(); err != nil {
		return nil, err
	}
	if receipt.Operation, err = r.u8(); err != nil {
		return nil, err
	}
	if receipt.Asset, err = r.array32(); err != nil {
		return nil, err
	}
	if receipt.Amount, err = r.u128(); err != nil {
		return nil, err
	}
	if receipt.From, err = r.array32(); err != nil {
		return nil, err
	}
	if receipt.FromBalanceBefore, err = r.u128(); err != nil {
		return nil, err
	}
	if receipt.FromBalanceAfter, err = r.u128(); err != nil {
		return nil, err
	}
	if receipt.FromSequence, err = r.u64(); err != nil {
		return nil, err
	}
	if receipt.To, err = r.array32(); err != nil {
		return nil, err
	}
	if receipt.ToBalanceBefore, err = r.u128(); err != nil {
		return nil, err
	}
	if receipt.ToBalanceAfter, err = r.u128(); err != nil {
		return nil, err
	}
	if receipt.TransferSetRoot, err = r.array32(); err != nil {
		return nil, err
	}
	if receipt.AuthorizationHash, err = r.array32(); err != nil {
		return nil, err
	}
	if receipt.ContextHash, err = r.array32(); err != nil {
		return nil, err
	}
	if receipt.Timestamp, err = r.u64(); err != nil {
		return nil, err
	}
	if supplyPresent {
		if receipt.Supply, err = decodeSupply(r, receipt); err != nil {
			return nil, err
		}
	}
	if r.remaining() > signatureBlockBytes {
		if receipt.ProgramOutcome, err = decodeProgramOutcome(r, receipt.ProtocolVersion); err != nil {
			return nil, err
		}
		o := receipt.ProgramOutcome
		if receipt.ModuleID != 9 || o.ResultCode != receipt.ResultCode ||
			(o.TerminalKind == 1 && o.TransferRoot != receipt.TransferSetRoot) ||
			(o.TerminalKind != 1 && receipt.TransferSetRoot != ([32]byte{})) {
			return nil, ErrFatalInvariant
		}
	}
	present, err := r.u8()
	if err != nil {
		return nil, err
	}
	switch present {
	case 0:
	case 1:
		signature, err := r.array64()
		if err != nil {
			return nil, err
		}
		receipt.SequencerSignature = &signature
	default:
		return nil, ErrNonCanonical
	}
	if err := r.finish(); err != nil {
		return nil, err
	}
	zero := [32]byte{}
	if receipt.GlobalSequence == 0 || receipt.ModuleID == 0 || receipt.ModuleVersion == 0 ||
		receipt.Timestamp == 0 || receipt.ActivityID == zero || receipt.ResultingStateRoot == zero {
		return nil, ErrNonCanonical
	}
	receipt.canonical = append([]byte(nil), bytes...)
	return receipt, nil
}
