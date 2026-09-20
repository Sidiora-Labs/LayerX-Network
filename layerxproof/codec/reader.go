// Package codec decodes the canonical LayerX wire structures byte-exactly with
// the C core (src/protocol, src/crypto) and the Rust crates layerx-wire and
// layerx-proof. Every decoder is strict: a malformed, truncated, oversize or
// trailing-byte input is an error and never a partial value.
package codec

import (
	"encoding/binary"
	"errors"
)

var (
	ErrTruncated          = errors.New("layerx codec: truncated")
	ErrLengthLimit        = errors.New("layerx codec: length limit")
	ErrNonCanonical       = errors.New("layerx codec: non-canonical")
	ErrInvalidTag         = errors.New("layerx codec: invalid tag")
	ErrVersionUnsupported = errors.New("layerx codec: version unsupported")
	ErrTrailingBytes      = errors.New("layerx codec: trailing bytes")
	ErrUnknownField       = errors.New("layerx codec: unknown field")
	ErrFatalInvariant     = errors.New("layerx codec: fatal invariant")
	ErrUnsortedSequence   = errors.New("layerx codec: unsorted sequence")
	ErrReceiptShape       = errors.New("layerx codec: not a full protocol receipt")
)

const (
	// MaxMessageBytes bounds one canonical activity or receipt.
	MaxMessageBytes = 1_048_576
	// LegacyProtocolVersion is retained for explicit decode compatibility.
	LegacyProtocolVersion uint16 = 1
	// OccupancyProtocolVersion is the default occupancy-layout protocol.
	OccupancyProtocolVersion uint16 = 2
	// StateCommitmentProtocolVersion is the composite-state protocol.
	StateCommitmentProtocolVersion uint16 = 3
	// StructureVersion heads structures that carry no protocol version.
	StructureVersion uint16 = 1
)

// ProtocolVersionUsesOccupancy reports the versions every verifier accepts.
func ProtocolVersionUsesOccupancy(version uint16) bool {
	return version == OccupancyProtocolVersion || version == StateCommitmentProtocolVersion
}

// U128 is a fixed-width big-endian unsigned 128-bit integer.
type U128 struct {
	Hi uint64
	Lo uint64
}

// IsZero reports whether the value is zero.
func (v U128) IsZero() bool { return v.Hi == 0 && v.Lo == 0 }

// Bytes returns the 16-byte big-endian form.
func (v U128) Bytes() [16]byte {
	var out [16]byte
	binary.BigEndian.PutUint64(out[:8], v.Hi)
	binary.BigEndian.PutUint64(out[8:], v.Lo)
	return out
}

// CheckedAdd returns v+o and false on overflow.
func (v U128) CheckedAdd(o U128) (U128, bool) {
	lo := v.Lo + o.Lo
	carry := uint64(0)
	if lo < v.Lo {
		carry = 1
	}
	hi := v.Hi + o.Hi
	if hi < v.Hi {
		return U128{}, false
	}
	hi2 := hi + carry
	if hi2 < hi {
		return U128{}, false
	}
	return U128{Hi: hi2, Lo: lo}, true
}

// CheckedSub returns v-o and false on underflow.
func (v U128) CheckedSub(o U128) (U128, bool) {
	if v.Hi < o.Hi || (v.Hi == o.Hi && v.Lo < o.Lo) {
		return U128{}, false
	}
	borrow := uint64(0)
	if v.Lo < o.Lo {
		borrow = 1
	}
	return U128{Hi: v.Hi - o.Hi - borrow, Lo: v.Lo - o.Lo}, true
}

type reader struct {
	bytes  []byte
	offset int
}

func (r *reader) remaining() int { return len(r.bytes) - r.offset }

func (r *reader) take(length int) ([]byte, error) {
	if length < 0 || length > r.remaining() {
		return nil, ErrTruncated
	}
	value := r.bytes[r.offset : r.offset+length]
	r.offset += length
	return value, nil
}

func (r *reader) u8() (uint8, error) {
	b, err := r.take(1)
	if err != nil {
		return 0, err
	}
	return b[0], nil
}

func (r *reader) u16() (uint16, error) {
	b, err := r.take(2)
	if err != nil {
		return 0, err
	}
	return binary.BigEndian.Uint16(b), nil
}

func (r *reader) u32() (uint32, error) {
	b, err := r.take(4)
	if err != nil {
		return 0, err
	}
	return binary.BigEndian.Uint32(b), nil
}

func (r *reader) u64() (uint64, error) {
	b, err := r.take(8)
	if err != nil {
		return 0, err
	}
	return binary.BigEndian.Uint64(b), nil
}

func (r *reader) i32() (int32, error) {
	v, err := r.u32()
	return int32(v), err //nolint:gosec
}

func (r *reader) u128() (U128, error) {
	b, err := r.take(16)
	if err != nil {
		return U128{}, err
	}
	return U128{Hi: binary.BigEndian.Uint64(b[:8]), Lo: binary.BigEndian.Uint64(b[8:])}, nil
}

// prefixed borrows a u32-length-prefixed byte string bounded by maximum.
func (r *reader) prefixed(maximum int) ([]byte, error) {
	length, err := r.u32()
	if err != nil {
		return nil, err
	}
	if uint64(length) > uint64(maximum) { //nolint:gosec
		return nil, ErrLengthLimit
	}
	return r.take(int(length))
}

// array32 reads a length-prefixed digest whose length must be exactly 32.
func (r *reader) array32() ([32]byte, error) {
	var out [32]byte
	b, err := r.prefixed(32)
	if err != nil {
		return out, err
	}
	if len(b) != 32 {
		return out, ErrNonCanonical
	}
	copy(out[:], b)
	return out, nil
}

func (r *reader) array64() ([64]byte, error) {
	var out [64]byte
	b, err := r.prefixed(64)
	if err != nil {
		return out, err
	}
	if len(b) != 64 {
		return out, ErrNonCanonical
	}
	copy(out[:], b)
	return out, nil
}

func (r *reader) fixed32() ([32]byte, error) {
	var out [32]byte
	b, err := r.take(32)
	if err != nil {
		return out, err
	}
	copy(out[:], b)
	return out, nil
}

func (r *reader) sequenceLength(maximum int) (int, error) {
	count, err := r.u32()
	if err != nil {
		return 0, err
	}
	if uint64(count) > uint64(maximum) { //nolint:gosec
		return 0, ErrLengthLimit
	}
	return int(count), nil
}

func (r *reader) structureHeader(expected uint16) error {
	version, err := r.u16()
	if err != nil {
		return err
	}
	tag, err := r.u16()
	if err != nil {
		return err
	}
	if version != StructureVersion || expected == 0 || tag != expected {
		return ErrVersionUnsupported
	}
	return nil
}

func (r *reader) structureHeaderVersion(expected uint16) (uint16, error) {
	version, err := r.u16()
	if err != nil {
		return 0, err
	}
	tag, err := r.u16()
	if err != nil {
		return 0, err
	}
	if version < LegacyProtocolVersion || version > StateCommitmentProtocolVersion || expected == 0 || tag != expected {
		return 0, ErrVersionUnsupported
	}
	return version, nil
}

func (r *reader) finish() error {
	if r.offset != len(r.bytes) {
		return ErrTrailingBytes
	}
	return nil
}
