package types

import (
	"encoding/hex"
	"encoding/json"
	"fmt"

	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// Hash32 is a 32-byte value rendered as hex in genesis and state JSON.
type Hash32 [32]byte

func (h Hash32) MarshalJSON() ([]byte, error) { return json.Marshal(hex.EncodeToString(h[:])) }

func (h *Hash32) UnmarshalJSON(raw []byte) error { return unmarshalFixedHex(raw, h[:]) }

func (h Hash32) Hex() string { return "0x" + hex.EncodeToString(h[:]) }

// Address20 is a 20-byte EVM address rendered as hex.
type Address20 [20]byte

func (a Address20) MarshalJSON() ([]byte, error) { return json.Marshal(hex.EncodeToString(a[:])) }

func (a *Address20) UnmarshalJSON(raw []byte) error { return unmarshalFixedHex(raw, a[:]) }

func (a Address20) Hex() string { return "0x" + hex.EncodeToString(a[:]) }

func unmarshalFixedHex(raw []byte, out []byte) error {
	var text string
	if err := json.Unmarshal(raw, &text); err != nil {
		return err
	}
	decoded, err := hex.DecodeString(text)
	if err != nil {
		return err
	}
	if len(decoded) != len(out) {
		return fmt.Errorf("expected %d bytes, got %d", len(out), len(decoded))
	}
	copy(out, decoded)
	return nil
}

// Request kinds.
const (
	KindFetch  uint8 = 1
	KindSearch uint8 = 2
)

// KnownKind reports whether kind is a request kind the module accepts.
func KnownKind(kind uint8) bool { return kind == KindFetch || kind == KindSearch }

// Documented parameter defaults. The module ships paused with an empty
// attestor set; governance sets the live values before unpausing.
const (
	DefaultMaxPayloadBytes = uint32(2048)
	DefaultMaxCallbackGas  = uint64(500_000)
	DefaultTimeoutBlocks   = uint64(3600)

	// PayloadBytesLimit and CallbackGasLimit bound what governance may set
	// the payload and callback caps to.
	PayloadBytesLimit = uint32(16384)
	CallbackGasLimit  = uint64(5_000_000)
)

// DefaultFee is one PAX in base units.
func DefaultFee() sdk.Int { return sdk.NewInt(1_000_000) }

// Params holds the module authority (the only account that may send the
// governance messages), the fee a request pays in the base denom, the payload
// and callback caps and the timeout in blocks after which an unfulfilled
// request is refundable.
type Params struct {
	Authority       string  `json:"authority"`
	Fee             sdk.Int `json:"fee"`
	MaxPayloadBytes uint32  `json:"max_payload_bytes"`
	MaxCallbackGas  uint64  `json:"max_callback_gas"`
	TimeoutBlocks   uint64  `json:"timeout_blocks"`
}

func DefaultParams(authority string) Params {
	return Params{
		Authority:       authority,
		Fee:             DefaultFee(),
		MaxPayloadBytes: DefaultMaxPayloadBytes,
		MaxCallbackGas:  DefaultMaxCallbackGas,
		TimeoutBlocks:   DefaultTimeoutBlocks,
	}
}

func (p Params) Validate() error {
	if _, err := sdk.AccAddressFromBech32(p.Authority); err != nil {
		return ErrInvalidParams.Wrapf("authority: %v", err)
	}
	return ValidateSettings(p.Fee, p.MaxPayloadBytes, p.MaxCallbackGas, p.TimeoutBlocks)
}

// ValidateSettings checks the governance-settable parameters.
func ValidateSettings(fee sdk.Int, maxPayloadBytes uint32, maxCallbackGas, timeoutBlocks uint64) error {
	if fee.IsNil() || !fee.IsPositive() {
		return ErrInvalidParams.Wrap("fee must be positive")
	}
	if maxPayloadBytes == 0 || maxPayloadBytes > PayloadBytesLimit {
		return ErrInvalidParams.Wrapf("payload cap %d outside 1..%d", maxPayloadBytes, PayloadBytesLimit)
	}
	if maxCallbackGas == 0 || maxCallbackGas > CallbackGasLimit {
		return ErrInvalidParams.Wrapf("callback cap %d outside 1..%d", maxCallbackGas, CallbackGasLimit)
	}
	if timeoutBlocks == 0 {
		return ErrInvalidParams.Wrap("timeout is zero")
	}
	return nil
}

// Attestor is one web attestor: the EVM address its secp256k1 signatures
// recover to and the bank account its share of each fee is paid to.
type Attestor struct {
	Signer Address20 `json:"signer"`
	Payout string    `json:"payout"`
}

func (a Attestor) Validate() error {
	if a.Signer == (Address20{}) {
		return ErrInvalidAttestors.Wrap("zero signer")
	}
	if _, err := sdk.AccAddressFromBech32(a.Payout); err != nil {
		return ErrInvalidAttestors.Wrapf("payout of %s: %v", a.Signer.Hex(), err)
	}
	return nil
}

// AttestorSet is the registered attestors and the number of distinct members
// whose signatures a fulfilment needs. A non-empty set requires a strict
// majority threshold; the empty set with threshold zero accepts nothing.
type AttestorSet struct {
	Attestors []Attestor `json:"attestors"`
	Threshold uint32     `json:"threshold"`
}

// Majority is the smallest threshold above half of n attestors.
func Majority(n int) uint32 { return uint32(n/2 + 1) }

// ValidThreshold reports whether threshold is above half of n and at most n.
func ValidThreshold(threshold uint32, n int) bool {
	return n > 0 && 2*int(threshold) > n && int(threshold) <= n
}

func (s AttestorSet) Validate() error {
	if len(s.Attestors) == 0 {
		if s.Threshold != 0 {
			return ErrInvalidAttestors.Wrap("threshold without attestors")
		}
		return nil
	}
	if len(s.Attestors) > MaxAttestors {
		return ErrInvalidAttestors.Wrapf("more than %d attestors", MaxAttestors)
	}
	if !ValidThreshold(s.Threshold, len(s.Attestors)) {
		return ErrInvalidThreshold.Wrapf("threshold %d of %d attestors", s.Threshold, len(s.Attestors))
	}
	seen := map[Address20]bool{}
	for _, attestor := range s.Attestors {
		if err := attestor.Validate(); err != nil {
			return err
		}
		if seen[attestor.Signer] {
			return ErrInvalidAttestors.Wrapf("duplicate signer %s", attestor.Signer.Hex())
		}
		seen[attestor.Signer] = true
	}
	return nil
}

// Find returns the registered attestor with signer.
func (s AttestorSet) Find(signer Address20) (Attestor, bool) {
	for _, attestor := range s.Attestors {
		if attestor.Signer == signer {
			return attestor, true
		}
	}
	return Attestor{}, false
}

// Has reports whether signer is a member of the set.
func (s AttestorSet) Has(signer Address20) bool {
	_, found := s.Find(signer)
	return found
}

// RequestStatus is where a request is in its life: pending until a
// fulfilment or a refund ends it, and never both.
type RequestStatus uint8

const (
	StatusPending   RequestStatus = 0
	StatusFulfilled RequestStatus = 1
	StatusRefunded  RequestStatus = 2
)

// Request is one contract request for web data, stored under its nonce.
type Request struct {
	ID            uint64        `json:"id"`
	Requester     Address20     `json:"requester"`
	Kind          uint8         `json:"kind"`
	PayloadHash   Hash32        `json:"payload_hash"`
	CallbackGas   uint64        `json:"callback_gas"`
	Fee           sdk.Int       `json:"fee"`
	Height        int64         `json:"height"`
	TimeoutHeight int64         `json:"timeout_height"`
	Status        RequestStatus `json:"status"`
}

func (r Request) Validate() error {
	if r.ID == 0 {
		return ErrInvalidRequest.Wrap("request id is zero")
	}
	if r.Requester == (Address20{}) {
		return ErrInvalidRequest.Wrapf("request %d has a zero requester", r.ID)
	}
	if !KnownKind(r.Kind) {
		return ErrUnknownKind.Wrapf("request %d kind %d", r.ID, r.Kind)
	}
	if r.CallbackGas == 0 {
		return ErrCallbackGas.Wrapf("request %d", r.ID)
	}
	if r.Fee.IsNil() || !r.Fee.IsPositive() {
		return ErrInvalidRequest.Wrapf("request %d fee must be positive", r.ID)
	}
	if r.Height < 0 || r.TimeoutHeight <= r.Height {
		return ErrInvalidRequest.Wrapf("request %d heights %d..%d", r.ID, r.Height, r.TimeoutHeight)
	}
	if r.Status > StatusRefunded {
		return ErrInvalidRequest.Wrapf("request %d status %d", r.ID, r.Status)
	}
	return nil
}

// CallbackOutcome records what the requester's onXWebResponse callback did.
// The fulfilment stands whatever the outcome.
type CallbackOutcome uint8

const (
	CallbackPending   CallbackOutcome = 0
	CallbackDelivered CallbackOutcome = 1
	CallbackReverted  CallbackOutcome = 2
	CallbackOutOfGas  CallbackOutcome = 3
)

// Result is the attested answer to one request.
type Result struct {
	RequestID       uint64          `json:"request_id"`
	Response        []byte          `json:"response"`
	ContentDigest   Hash32          `json:"content_digest"`
	FullLength      uint32          `json:"full_length"`
	Signers         []Address20     `json:"signers"`
	Height          int64           `json:"height"`
	Callback        CallbackOutcome `json:"callback"`
	CallbackGasUsed uint64          `json:"callback_gas_used"`
}

func (r Result) Validate() error {
	if r.RequestID == 0 {
		return ErrInvalidRequest.Wrap("result of request id zero")
	}
	if len(r.Response) > MaxResponseBytes {
		return ErrResponseTooLarge.Wrapf("result %d holds %d bytes", r.RequestID, len(r.Response))
	}
	if uint64(r.FullLength) < uint64(len(r.Response)) {
		return ErrInvalidLength.Wrapf("result %d", r.RequestID)
	}
	if len(r.Signers) == 0 {
		return ErrInvalidRequest.Wrapf("result %d has no signers", r.RequestID)
	}
	if r.Callback > CallbackOutOfGas {
		return ErrInvalidRequest.Wrapf("result %d callback outcome %d", r.RequestID, r.Callback)
	}
	return nil
}
