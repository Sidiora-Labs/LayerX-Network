package layerx

import (
	"bytes"
	"encoding/binary"
	"sort"
)

const MaximumNativeCapabilities = 238
const MaximumNativeCapabilityBytes = 65452
const MaximumNativeBalanceViews = 32

type NativeCapability interface{ nativeCapability() }
type NativeStorageRead struct{}
type NativeStorageWrite struct{}
type NativeSharedStorageRead struct{}
type NativeSharedStorageWrite struct{}
type NativeEmitEvent struct{}
type NativeCallCapability struct{ Program [32]byte }
type NativeTransfer402 struct {
	Asset, To     [32]byte
	MaximumAmount Uint128
}
type NativeProgramSpend struct {
	OwnerProgram             [32]byte
	Seed                     []byte
	SourceAccount, Asset, To [32]byte
	MaximumAmount            Uint128
}
type NativeReceiptRead struct{ ReceiptDigest [32]byte }
type NativeBalanceView struct{ Account, Asset, ReceiptDigest [32]byte }

func DeriveNativeProgramAccount(owner [32]byte, seed []byte) ([32]byte, error) {
	if owner == ([32]byte{}) || len(seed) > 128 {
		return [32]byte{}, newSDKError(ErrorInvalidArgument, RetryNever)
	}
	return deriveProgramAccount(owner, seed), nil
}

func (NativeStorageRead) nativeCapability()        {}
func (NativeStorageWrite) nativeCapability()       {}
func (NativeSharedStorageRead) nativeCapability()  {}
func (NativeSharedStorageWrite) nativeCapability() {}
func (NativeEmitEvent) nativeCapability()          {}
func (NativeCallCapability) nativeCapability()     {}
func (NativeTransfer402) nativeCapability()        {}
func (NativeProgramSpend) nativeCapability()       {}
func (NativeReceiptRead) nativeCapability()        {}
func (NativeBalanceView) nativeCapability()        {}

type nativeCapabilityEntry struct {
	rank    int
	key     [][]byte
	encoded []byte
	maximum Uint128
	receipt [32]byte
}

func nativeCapabilityEntryFor(grant NativeCapability) (nativeCapabilityEntry, error) {
	entry := nativeCapabilityEntry{}
	invalid := newSDKError(ErrorInvalidArgument, RetryNever)
	put := func(fields ...[32]byte) bool {
		for _, field := range fields {
			if field == ([32]byte{}) {
				return false
			}
			entry.encoded = append(entry.encoded, field[:]...)
			entry.key = append(entry.key, append([]byte{}, field[:]...))
		}
		return true
	}
	amount := func(value Uint128) bool {
		if value == (Uint128{}) {
			return false
		}
		entry.maximum = value
		var encoded [16]byte
		binary.BigEndian.PutUint64(encoded[:8], value.High())
		binary.BigEndian.PutUint64(encoded[8:], value.Low())
		entry.encoded = append(entry.encoded, encoded[:]...)
		return true
	}
	switch value := grant.(type) {
	case NativeStorageRead:
		entry.rank, entry.encoded = 0, []byte{1}
	case NativeStorageWrite:
		entry.rank, entry.encoded = 1, []byte{2}
	case NativeEmitEvent:
		entry.rank, entry.encoded = 2, []byte{3}
	case NativeCallCapability:
		entry.rank, entry.encoded = 3, []byte{4}
		if !put(value.Program) {
			return entry, invalid
		}
	case NativeTransfer402:
		entry.rank, entry.encoded = 4, []byte{5}
		if !put(value.Asset, value.To) || !amount(value.MaximumAmount) {
			return entry, invalid
		}
	case NativeProgramSpend:
		entry.rank, entry.encoded = 5, []byte{9}
		if len(value.Seed) > 128 || !put(value.OwnerProgram) || value.SourceAccount != deriveProgramAccount(value.OwnerProgram, value.Seed) {
			return entry, invalid
		}
		entry.encoded = binary.BigEndian.AppendUint16(entry.encoded, uint16(len(value.Seed)))
		entry.encoded = append(entry.encoded, value.Seed...)
		entry.key = append(entry.key, append([]byte{}, value.Seed...))
		entry.encoded = append(entry.encoded, value.SourceAccount[:]...)
		entry.key = append(entry.key, append([]byte{}, value.SourceAccount[:]...))
		if !put(value.Asset, value.To) || !amount(value.MaximumAmount) {
			return entry, invalid
		}
	case NativeReceiptRead:
		entry.rank, entry.encoded = 6, []byte{6}
		if !put(value.ReceiptDigest) {
			return entry, invalid
		}
	case NativeBalanceView:
		entry.rank, entry.encoded = 7, []byte{10}
		if !put(value.Account, value.Asset) || value.ReceiptDigest == ([32]byte{}) {
			return entry, invalid
		}
		entry.receipt = value.ReceiptDigest
		entry.encoded = append(entry.encoded, value.ReceiptDigest[:]...)
	case NativeSharedStorageRead:
		entry.rank, entry.encoded = 8, []byte{7}
	case NativeSharedStorageWrite:
		entry.rank, entry.encoded = 9, []byte{8}
	default:
		return entry, invalid
	}
	return entry, nil
}

func compareNativeCapability(left, right nativeCapabilityEntry) int {
	if left.rank < right.rank {
		return -1
	}
	if left.rank > right.rank {
		return 1
	}
	return compareProgramCapabilityKey(left.key, right.key)
}

func EncodeNativeCapabilities(grants []NativeCapability) ([]byte, error) {
	invalid := newSDKError(ErrorInvalidArgument, RetryNever)
	if len(grants) > MaximumNativeCapabilities {
		return nil, invalid
	}
	entries := make([]nativeCapabilityEntry, 0, len(grants))
	balanceViews := 0
	for _, grant := range grants {
		entry, err := nativeCapabilityEntryFor(grant)
		if err != nil {
			return nil, err
		}
		if entry.rank == 7 {
			balanceViews++
		}
		entries = append(entries, entry)
	}
	if balanceViews > MaximumNativeBalanceViews {
		return nil, invalid
	}
	sort.Slice(entries, func(left, right int) bool { return compareNativeCapability(entries[left], entries[right]) < 0 })
	encoded := binary.BigEndian.AppendUint16(nil, uint16(len(entries)))
	for index, entry := range entries {
		if index > 0 && compareNativeCapability(entries[index-1], entry) == 0 {
			return nil, invalid
		}
		encoded = append(encoded, entry.encoded...)
	}
	if len(encoded) > MaximumNativeCapabilityBytes {
		return nil, invalid
	}
	return encoded, nil
}

func DecodeNativeCapabilities(encoded []byte) ([]NativeCapability, error) {
	invalid := newSDKError(ErrorInvalidArgument, RetryNever)
	if len(encoded) < 2 || len(encoded) > MaximumNativeCapabilityBytes {
		return nil, invalid
	}
	cursor := programTerminalCursor{value: encoded}
	count := cursor.u16()
	if count > MaximumNativeCapabilities {
		return nil, invalid
	}
	grants := make([]NativeCapability, 0, count)
	for index := uint16(0); index < count; index++ {
		var grant NativeCapability
		switch cursor.byte() {
		case 1:
			grant = NativeStorageRead{}
		case 2:
			grant = NativeStorageWrite{}
		case 3:
			grant = NativeEmitEvent{}
		case 4:
			grant = NativeCallCapability{cursor.array32()}
		case 5:
			grant = NativeTransfer402{cursor.array32(), cursor.array32(), cursor.u128()}
		case 6:
			grant = NativeReceiptRead{cursor.array32()}
		case 7:
			grant = NativeSharedStorageRead{}
		case 8:
			grant = NativeSharedStorageWrite{}
		case 9:
			owner := cursor.array32()
			length := cursor.u16()
			if length > 128 {
				return nil, invalid
			}
			grant = NativeProgramSpend{owner, append([]byte{}, cursor.take(int(length))...), cursor.array32(), cursor.array32(), cursor.array32(), cursor.u128()}
		case 10:
			grant = NativeBalanceView{cursor.array32(), cursor.array32(), cursor.array32()}
		default:
			return nil, invalid
		}
		if cursor.failed {
			return nil, invalid
		}
		grants = append(grants, grant)
	}
	canonical, err := EncodeNativeCapabilities(grants)
	if err != nil || !cursor.finished() || !bytes.Equal(canonical, encoded) {
		return nil, invalid
	}
	return grants, nil
}

func NarrowNativeCapabilities(parent, requested []NativeCapability) ([]NativeCapability, error) {
	parentBytes, err := EncodeNativeCapabilities(parent)
	if err != nil {
		return nil, err
	}
	parents, err := DecodeNativeCapabilities(parentBytes)
	if err != nil {
		return nil, err
	}
	requestedBytes, err := EncodeNativeCapabilities(requested)
	if err != nil {
		return nil, err
	}
	children, err := DecodeNativeCapabilities(requestedBytes)
	if err != nil {
		return nil, err
	}
	for _, child := range children {
		entry, err := nativeCapabilityEntryFor(child)
		if err != nil {
			return nil, err
		}
		found := false
		for _, grant := range parents {
			ancestor, err := nativeCapabilityEntryFor(grant)
			if err != nil {
				return nil, err
			}
			if compareNativeCapability(ancestor, entry) != 0 {
				continue
			}
			found = true
			if entry.maximum.High() > ancestor.maximum.High() || entry.maximum.High() == ancestor.maximum.High() && entry.maximum.Low() > ancestor.maximum.Low() || entry.receipt != ancestor.receipt {
				return nil, newSDKError(ErrorInvalidArgument, RetryNever)
			}
			break
		}
		if !found {
			return nil, newSDKError(ErrorInvalidArgument, RetryNever)
		}
	}
	return children, nil
}
