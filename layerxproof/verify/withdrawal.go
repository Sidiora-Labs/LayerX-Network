package verify

import (
	"bytes"
	"crypto/sha256"
	"encoding/binary"
	"errors"

	"github.com/sidiora-labs/paxeer-network/layerxproof/codec"
)

const (
	// WithdrawalModuleID is the native asset module.
	WithdrawalModuleID uint16 = 1
	// WithdrawalOperation is the native Paxeer withdrawal activity ordinal.
	WithdrawalOperation uint8 = 9
	// WithdrawalEventBytes is the exact width of the withdrawal event body.
	WithdrawalEventBytes = 254
	// WithdrawalPayloadBytes is the exact width of the original request payload.
	WithdrawalPayloadBytes = 108
	// WithdrawalAccountName is the native account every withdrawal credits.
	WithdrawalAccountName = "system:paxeer-withdrawals"
	// WithdrawalNullifierDomain prefixes the withdrawal nullifier preimage.
	WithdrawalNullifierDomain = "LX:WITHDRAWAL:v1"

	withdrawalProtocolVersion uint16 = 3
	withdrawalEventVersion    uint16 = 2
)

// ErrWithdrawalEffect refuses a receipt that is not an exact successful native
// withdrawal (layerx-proof receipt::withdrawal refuses with ReceiptShape).
var ErrWithdrawalEffect = errors.New("layerx verify: receipt is not a native withdrawal outcome")

// WithdrawalEffect is the withdrawal a successful native withdrawal receipt
// commits to, decoded from its event effect.
type WithdrawalEffect struct {
	NetworkID uint32
	// WithdrawalID is the activity identifier of the withdrawal request.
	WithdrawalID [32]byte
	// Account is the debited LayerX account.
	Account [32]byte
	Asset   [32]byte
	Amount  codec.U128
	// Recipient is the Paxeer EVM address that is paid.
	Recipient [20]byte
	// Anchor is the request anchor the nullifier binds (the checkpoint hash
	// field of PaxeerWithdrawalCodec.nullifier).
	Anchor         [32]byte
	PayloadHash    [32]byte
	IdempotencyKey [32]byte
	FeeLimit       uint64
	// Nullifier is SHA256("LX:WITHDRAWAL:v1" || network || withdrawal id ||
	// account || asset || amount || anchor), equal to the receipt context hash.
	Nullifier [32]byte
}

func allZero(b []byte) bool {
	for _, x := range b {
		if x != 0 {
			return false
		}
	}
	return true
}

func withdrawalShape(r *codec.Receipt) bool {
	debit, debitOK := r.FromBalanceBefore.CheckedSub(r.Amount)
	credit, creditOK := r.ToBalanceBefore.CheckedAdd(r.Amount)
	return r.ProtocolVersion == withdrawalProtocolVersion &&
		r.ModuleID == WithdrawalModuleID &&
		r.ModuleVersion == 1 &&
		r.Operation == WithdrawalOperation &&
		r.ResultCode == 0 &&
		r.ActivityID != ([32]byte{}) &&
		r.ParameterVersion != 0 &&
		!r.Amount.IsZero() &&
		r.Asset != ([32]byte{}) &&
		r.From != ([32]byte{}) &&
		r.To != ([32]byte{}) &&
		r.From != r.To &&
		debitOK && debit == r.FromBalanceAfter &&
		creditOK && credit == r.ToBalanceAfter &&
		r.AuthorizationHash != ([32]byte{}) &&
		r.ContextHash != ([32]byte{}) &&
		r.TransferSetRoot != ([32]byte{}) &&
		r.ProgramOutcome == nil
}

// withdrawalPayload reproduces the original 108-byte request payload from the
// event body and binds it to the payload hash the body carries.
func withdrawalPayload(body []byte) ([WithdrawalPayloadBytes]byte, bool) {
	var payload [WithdrawalPayloadBytes]byte
	if len(body) != WithdrawalEventBytes || binary.BigEndian.Uint16(body[:2]) != withdrawalEventVersion ||
		!allZero(body[118:130]) {
		return payload, false
	}
	copy(payload[:32], body[70:102])
	copy(payload[32:48], body[102:118])
	copy(payload[48:68], body[130:150])
	copy(payload[68:100], body[150:182])
	copy(payload[100:], body[246:])
	for _, span := range [][2]int{{2, 6}, {6, 38}, {38, 70}, {70, 102}, {102, 118}, {130, 150}, {150, 182}} {
		if allZero(body[span[0]:span[1]]) {
			return payload, false
		}
	}
	hash, err := codec.DomainHash(codec.DomainPayloadHash, payload[:])
	if err != nil || !bytes.Equal(hash[:], body[182:214]) {
		return payload, false
	}
	return payload, true
}

// WithdrawalOutcome is the Go port of layerx-proof
// receipt::withdrawal::verify_effects over an already authenticated receipt:
// the exact successful native withdrawal shape, its transfer and event
// effects, the payload hash, the nullifier bound as context hash, and the
// single transfer leg to system:paxeer-withdrawals. It does not authenticate
// the receipt; pair it with ReceiptSignature, ReceiptAtRoot or
// ReceiptInclusion.
func WithdrawalOutcome(r *codec.Receipt) (*WithdrawalEffect, error) {
	if r == nil || !withdrawalShape(r) || len(r.Effects) != 2 {
		return nil, ErrWithdrawalEffect
	}
	transfer, event := r.Effects[0], r.Effects[1]
	if transfer.ModuleID != WithdrawalModuleID || transfer.Ordinal != 0 || transfer.Kind != 2 ||
		!transfer.Monetary || transfer.EventType != 0 || len(transfer.Body) != 0 ||
		event.ModuleID != WithdrawalModuleID || event.Ordinal != 1 || event.Kind != 3 ||
		event.Monetary || event.EventType != uint16(WithdrawalOperation) ||
		event.TransferSetRoot != ([32]byte{}) {
		return nil, ErrWithdrawalEffect
	}
	body := event.Body
	payload, ok := withdrawalPayload(body)
	if !ok {
		return nil, ErrWithdrawalEffect
	}
	feeLimit := binary.BigEndian.Uint64(payload[100:])
	if !bytes.Equal(body[6:38], r.ActivityID[:]) || r.FeeCharged.Hi != 0 || r.FeeCharged.Lo > feeLimit {
		return nil, ErrWithdrawalEffect
	}
	destination, err := codec.DeriveAccountID([]byte(WithdrawalAccountName))
	if err != nil {
		return nil, ErrWithdrawalEffect
	}
	var leg [115]byte
	copy(leg[1:33], body[38:70])
	copy(leg[33:65], destination[:])
	copy(leg[65:97], payload[:32])
	copy(leg[97:113], payload[32:48])
	binary.BigEndian.PutUint16(leg[113:], withdrawalProtocolVersion)
	hasher := sha256.New()
	hasher.Write([]byte(WithdrawalNullifierDomain))
	hasher.Write(body[2:118])
	hasher.Write(body[150:182])
	var nullifier [32]byte
	hasher.Sum(nullifier[:0])
	amount := r.Amount.Bytes()
	if !bytes.Equal(r.Asset[:], body[70:102]) ||
		!bytes.Equal(r.From[:], body[38:70]) ||
		r.To != destination ||
		!bytes.Equal(amount[:], payload[32:48]) ||
		r.ContextHash != nullifier ||
		r.TransferSetRoot != transfer.TransferSetRoot ||
		codec.MerkleLeafHash(leg[:]) != transfer.TransferSetRoot {
		return nil, ErrWithdrawalEffect
	}
	effect := &WithdrawalEffect{
		NetworkID: binary.BigEndian.Uint32(body[2:6]),
		Amount:    r.Amount,
		FeeLimit:  feeLimit,
		Nullifier: nullifier,
	}
	copy(effect.WithdrawalID[:], body[6:38])
	copy(effect.Account[:], body[38:70])
	copy(effect.Asset[:], body[70:102])
	copy(effect.Recipient[:], body[130:150])
	copy(effect.Anchor[:], body[150:182])
	copy(effect.PayloadHash[:], body[182:214])
	copy(effect.IdempotencyKey[:], body[214:246])
	return effect, nil
}

// WithdrawalReceipt authenticates receipt bytes under the sequencer key and
// verifies the native withdrawal outcome they carry.
func WithdrawalReceipt(receiptBytes []byte, sequencerPublicKey [32]byte) (*VerifiedReceipt, *WithdrawalEffect, error) {
	verified, err := ReceiptSignature(receiptBytes, sequencerPublicKey)
	if err != nil {
		return nil, nil, err
	}
	effect, err := WithdrawalOutcome(verified.Receipt)
	if err != nil {
		return nil, nil, err
	}
	return verified, effect, nil
}

// WithdrawalInclusion proves a native withdrawal receipt included in a
// sequencer-signed batch header and returns the withdrawal it commits to. The
// withdrawal must name the header's network.
func WithdrawalInclusion(receiptBytes []byte, proof *codec.MerkleProof, headerBytes []byte, headerSignature [64]byte,
	authorization SequencerAuthorization) (*VerifiedReceipt, *VerifiedBatchHeader, *WithdrawalEffect, error) {
	verified, header, err := ReceiptInclusion(receiptBytes, proof, headerBytes, headerSignature, authorization)
	if err != nil {
		return nil, nil, nil, err
	}
	effect, err := WithdrawalOutcome(verified.Receipt)
	if err != nil {
		return nil, nil, nil, err
	}
	if effect.NetworkID != header.Header.NetworkID {
		return nil, nil, nil, ErrWithdrawalEffect
	}
	return verified, header, effect, nil
}

// ExitRecipientDomain prefixes the forced-exit recipient authorisation.
const ExitRecipientDomain = "LX:SETTLE:RECIPIENT:v1"

// ExitRecipientMessage is the message an account authority signs to name the
// Paxeer recipient of a forced exit, byte for byte the preimage of
// NativeStateProof.verifyBalance.
func ExitRecipientMessage(networkID uint32, account, asset [32]byte, recipient [20]byte, anchor [32]byte) []byte {
	out := make([]byte, 0, len(ExitRecipientDomain)+1+4+32+32+20+32)
	out = append(out, ExitRecipientDomain...)
	out = append(out, 0)
	out = binary.BigEndian.AppendUint32(out, networkID)
	out = append(out, account[:]...)
	out = append(out, asset[:]...)
	out = append(out, recipient[:]...)
	return append(out, anchor[:]...)
}

// ErrExitAuthority refuses a forced exit the account authority did not sign.
var ErrExitAuthority = errors.New("layerx verify: forced exit recipient authorisation")

// ExitBalance proves an account balance under a finalized state root and that
// the account's authority key named recipient for anchor.
func ExitBalance(witnessBytes []byte, stateRoot [32]byte, networkID uint32, account, asset [32]byte,
	recipient [20]byte, anchor [32]byte, recipientSignature [64]byte) (*codec.Account, error) {
	proven, err := AccountProof(witnessBytes, stateRoot, account, &asset)
	if err != nil {
		return nil, err
	}
	if !proven.HasAuthorityKey || networkID == 0 || recipient == ([20]byte{}) || anchor == ([32]byte{}) {
		return nil, ErrExitAuthority
	}
	if err := Ed25519(proven.AuthorityKey, recipientSignature,
		ExitRecipientMessage(networkID, account, asset, recipient, anchor)); err != nil {
		return nil, ErrExitAuthority
	}
	return proven, nil
}
