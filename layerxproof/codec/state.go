package codec

import (
	"bytes"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"errors"
)

const (
	stateWitnessVersion uint16 = 2
	// MaxStateKeyBytes bounds a state witness key.
	MaxStateKeyBytes = 129
	// MaxStateValueBytes bounds a state witness value.
	MaxStateValueBytes = 1_048_576
	// MaxStateModuleID is the highest module with a composite subtree.
	MaxStateModuleID uint16 = 9
	// MaxAccountNameBytes bounds a canonical account name.
	MaxAccountNameBytes  = 512
	minAccountValueBytes = 103

	accountIdentifierDomain = "LX:ACCOUNT:v1"
	accountTreeKey          = "account-tree"
)

var (
	ErrStateVersion  = errors.New("layerx state proof: version")
	ErrStateEncoding = errors.New("layerx state proof: encoding")
	ErrStateModule   = errors.New("layerx state proof: module")
	ErrStatePath     = errors.New("layerx state proof: path")
	ErrStateRoot     = errors.New("layerx state proof: root")

	ErrAccountEncoding = errors.New("layerx account: encoding")
	ErrAccountIdentity = errors.New("layerx account: identity")
)

// StatePath is one index-aware path inside a state subtree.
type StatePath struct {
	Index    uint32
	Count    uint32
	Siblings [][32]byte
}

// StateWitness is the version-2 native state proof: a key/value leaf, its
// optional account-tree path, its module subtree path and the path of the
// module leaf in the composite state tree.
type StateWitness struct {
	ModuleID    uint16
	Key         []byte
	Value       []byte
	AccountPath *StatePath
	LeafIndexA  uint32
	LeafCountA  uint32
	SiblingsA   [][32]byte
	LeafCountB  uint32
	SiblingsB   [][32]byte
}

// StateLeafHash hashes one key/value pair under the state-leaf domain.
func StateLeafHash(key, value []byte) [32]byte {
	var lengths [8]byte
	binary.BigEndian.PutUint32(lengths[:4], uint32(len(key)))   //nolint:gosec
	binary.BigEndian.PutUint32(lengths[4:], uint32(len(value))) //nolint:gosec
	return mustDomainHash(DomainStateLeaf, lengths[:], key, value)
}

// StateNodeHash hashes two children under the state-node domain.
func StateNodeHash(left, right [32]byte) [32]byte {
	return mustDomainHash(DomainStateNode, left[:], right[:])
}

// Nodes returns the number of Merkle nodes the witness folds.
func (w *StateWitness) Nodes() int {
	nodes := len(w.SiblingsA) + len(w.SiblingsB)
	if w.AccountPath != nil {
		nodes += len(w.AccountPath.Siblings)
	}
	return nodes
}

func isAccountKey(moduleID uint16, key []byte) bool {
	return moduleID == 0 && len(key) == 33 && key[0] == 4
}

func readStatePath(r *reader) ([][32]byte, error) {
	depth, err := r.u8()
	if err != nil {
		return nil, ErrStateEncoding
	}
	if int(depth) > MaxProofDepth {
		return nil, ErrStatePath
	}
	path := make([][32]byte, depth)
	for level := range path {
		if path[level], err = r.fixed32(); err != nil {
			return nil, ErrStateEncoding
		}
	}
	return path, nil
}

func readStateVector(r *reader, maximum int) ([]byte, error) {
	length, err := r.u32()
	if err != nil || uint64(length) > uint64(maximum) { //nolint:gosec
		return nil, ErrStateEncoding
	}
	value, err := r.take(int(length))
	if err != nil {
		return nil, ErrStateEncoding
	}
	return append([]byte(nil), value...), nil
}

// DecodeStateWitness strictly decodes one native state proof and refuses any
// witness whose paths are not canonical, exactly as StateWitness::decode.
func DecodeStateWitness(input []byte) (*StateWitness, error) {
	if len(input) > MaxStateWitnessBytes {
		return nil, ErrStateEncoding
	}
	r := &reader{bytes: input}
	version, err := r.u16()
	if err != nil {
		return nil, ErrStateEncoding
	}
	if version != stateWitnessVersion {
		return nil, ErrStateVersion
	}
	w := &StateWitness{}
	if w.ModuleID, err = r.u16(); err != nil {
		return nil, ErrStateEncoding
	}
	if w.ModuleID > MaxStateModuleID {
		return nil, ErrStateModule
	}
	if w.Key, err = readStateVector(r, MaxStateKeyBytes); err != nil {
		return nil, err
	}
	if w.Value, err = readStateVector(r, MaxStateValueBytes); err != nil {
		return nil, err
	}
	if isAccountKey(w.ModuleID, w.Key) {
		path := &StatePath{}
		if path.Index, err = r.u32(); err != nil {
			return nil, ErrStateEncoding
		}
		if path.Count, err = r.u32(); err != nil {
			return nil, ErrStateEncoding
		}
		if path.Siblings, err = readStatePath(r); err != nil {
			return nil, err
		}
		w.AccountPath = path
	}
	if w.LeafIndexA, err = r.u32(); err != nil {
		return nil, ErrStateEncoding
	}
	if w.LeafCountA, err = r.u32(); err != nil {
		return nil, ErrStateEncoding
	}
	if w.SiblingsA, err = readStatePath(r); err != nil {
		return nil, err
	}
	if w.LeafCountB, err = r.u32(); err != nil {
		return nil, ErrStateEncoding
	}
	if w.SiblingsB, err = readStatePath(r); err != nil {
		return nil, err
	}
	if r.remaining() != 0 {
		return nil, ErrStateEncoding
	}
	if _, err := w.Root(); err != nil {
		return nil, err
	}
	return w, nil
}

// MaxStateWitnessBytes is the largest canonical state witness.
const MaxStateWitnessBytes = 4 + 4 + MaxStateKeyBytes + 4 + MaxStateValueBytes + 3*(8+1+MaxProofDepth*32)

func foldState(node [32]byte, index, count uint32, siblings [][32]byte) ([32]byte, error) {
	if count == 0 || index >= count || len(siblings) > MaxProofDepth {
		return [32]byte{}, ErrStatePath
	}
	for _, sibling := range siblings {
		if count <= 1 || ((index^1) >= count && sibling != node) {
			return [32]byte{}, ErrStatePath
		}
		if index&1 == 0 {
			node = StateNodeHash(node, sibling)
		} else {
			node = StateNodeHash(sibling, node)
		}
		index /= 2
		count = count/2 + count%2
	}
	if count != 1 {
		return [32]byte{}, ErrStatePath
	}
	return node, nil
}

// Root recomputes the composite state root the witness commits to.
func (w *StateWitness) Root() ([32]byte, error) {
	if w.ModuleID > MaxStateModuleID || w.LeafCountB < 9 || w.LeafCountB > 10 {
		return [32]byte{}, ErrStateModule
	}
	if len(w.Key) == 0 || len(w.Key) > MaxStateKeyBytes || len(w.Value) > MaxStateValueBytes {
		return [32]byte{}, ErrStateEncoding
	}
	if isAccountKey(w.ModuleID, w.Key) != (w.AccountPath != nil) {
		return [32]byte{}, ErrStateEncoding
	}
	node := StateLeafHash(w.Key, w.Value)
	var err error
	if w.AccountPath != nil {
		if node, err = foldState(node, w.AccountPath.Index, w.AccountPath.Count, w.AccountPath.Siblings); err != nil {
			return [32]byte{}, err
		}
		node = StateLeafHash([]byte(accountTreeKey), node[:])
	}
	subtree, err := foldState(node, w.LeafIndexA, w.LeafCountA, w.SiblingsA)
	if err != nil {
		return [32]byte{}, err
	}
	var module [2]byte
	binary.BigEndian.PutUint16(module[:], w.ModuleID)
	return foldState(StateLeafHash(module[:], subtree[:]), uint32(w.ModuleID), w.LeafCountB, w.SiblingsB)
}

// Verify refuses a witness whose recomputed root is not the trusted state root.
func (w *StateWitness) Verify(stateRoot [32]byte) error {
	root, err := w.Root()
	if err != nil {
		return err
	}
	if root != stateRoot {
		return ErrStateRoot
	}
	return nil
}

// Account is the canonical account value committed by lx_account_registry_root.
type Account struct {
	AccountID         [32]byte
	Name              []byte
	Kind              uint8
	HasAsset          bool
	AssetID           [32]byte
	Balance           U128
	NextSequence      uint64
	CreatedAtSequence uint64
	Frozen            bool
	HasOpenReference  bool
	HasAuthorityKey   bool
	AuthorityKey      [32]byte
}

func canonicalAccountName(name []byte) bool {
	if len(name) == 0 || len(name) > MaxAccountNameBytes {
		return false
	}
	previousColon := true
	for _, b := range name {
		valid := (b >= 'a' && b <= 'z') || (b >= '0' && b <= '9') || b == '.' || b == '_' || b == '-' || b == ':'
		if !valid || (b == ':' && previousColon) {
			return false
		}
		previousColon = b == ':'
	}
	return !previousColon
}

func agentShape(name, marker []byte) bool {
	if !bytes.HasPrefix(name, []byte("agent:")) || len(name) <= 6+len(marker) {
		return false
	}
	tail := name[6:]
	for offset := 1; offset+len(marker) <= len(tail); offset++ {
		if bytes.Equal(tail[offset:offset+len(marker)], marker) {
			rest := tail[offset+len(marker):]
			if len(rest) > 0 && !bytes.Contains(rest, []byte(":")) {
				return true
			}
		}
	}
	return false
}

func systemFunding(name, suffix []byte) bool {
	prefix := []byte("system:funding:")
	return len(name) > len(prefix)+len(suffix) && bytes.HasPrefix(name, prefix) && bytes.HasSuffix(name, suffix) &&
		!bytes.Contains(name[len(prefix):len(name)-len(suffix)], []byte(":"))
}

func moduleValue(name []byte) bool {
	prefix, marker := []byte("module:"), []byte(":value:")
	const identifierBytes = 64
	if len(name) <= len(prefix)+len(marker)+identifierBytes || !bytes.HasPrefix(name, prefix) ||
		!bytes.Equal(name[len(name)-identifierBytes-len(marker):len(name)-identifierBytes], marker) {
		return false
	}
	module := name[len(prefix) : len(name)-identifierBytes-len(marker)]
	if len(module) == 0 || len(module) > 31 {
		return false
	}
	for _, b := range module {
		if !((b >= 'a' && b <= 'z') || (b >= '0' && b <= '9') || b == '-') {
			return false
		}
	}
	for _, b := range name[len(name)-identifierBytes:] {
		if !((b >= '0' && b <= '9') || (b >= 'a' && b <= 'f')) {
			return false
		}
	}
	return true
}

// AccountKind classifies a canonical account name, reporting false for any
// name outside the native namespaces.
func AccountKind(name []byte) (uint8, bool) {
	if !canonicalAccountName(name) {
		return 0, false
	}
	liquidity := []byte("system:liquidity:")
	switch {
	case string(name) == "system:insurance":
		return 9, true
	case string(name) == "system:fees":
		return 10, true
	case string(name) == "system:paxeer-reserve":
		return 11, true
	case string(name) == "system:paxeer-withdrawals":
		return 12, true
	case len(name) > len(liquidity) && bytes.HasPrefix(name, liquidity) && !bytes.Contains(name[len(liquidity):], []byte(":")):
		return 6, true
	case systemFunding(name, []byte(":long")):
		return 7, true
	case systemFunding(name, []byte(":short")):
		return 8, true
	case bytes.HasPrefix(name, []byte("agent:")) && len(name) > 11 && bytes.HasSuffix(name, []byte(":main")):
		return 1, true
	case agentShape(name, []byte(":budget:")):
		return 2, true
	case agentShape(name, []byte(":escrow:")):
		return 3, true
	case agentShape(name, []byte(":stream:")):
		return 4, true
	case agentShape(name, []byte(":margin:")):
		return 5, true
	case moduleValue(name):
		return 13, true
	}
	return 0, false
}

// DeriveAccountID derives the native account identifier of a canonical name.
func DeriveAccountID(name []byte) ([32]byte, error) {
	var out [32]byte
	kind, ok := AccountKind(name)
	if !ok {
		return out, ErrAccountIdentity
	}
	if kind == 13 {
		if _, err := hex.Decode(out[:], name[len(name)-64:]); err != nil {
			return out, ErrAccountIdentity
		}
		return out, nil
	}
	var length [4]byte
	binary.BigEndian.PutUint32(length[:], uint32(len(name))) //nolint:gosec
	h := sha256.New()
	h.Write([]byte(accountIdentifierDomain))
	h.Write(length[:])
	h.Write(name)
	h.Sum(out[:0])
	return out, nil
}

func readBool(r *reader) (bool, error) {
	b, err := r.u8()
	if err != nil || b > 1 {
		return false, ErrAccountEncoding
	}
	return b == 1, nil
}

// DecodeAccountValue decodes the exact account value committed under the
// account key 0x04 || account_id and binds it to that identifier.
func DecodeAccountValue(accountID [32]byte, value []byte) (*Account, error) {
	if len(value) < minAccountValueBytes || len(value) > 2+MaxAccountNameBytes+minAccountValueBytes {
		return nil, ErrAccountEncoding
	}
	r := &reader{bytes: value}
	nameLength, err := r.u16()
	if err != nil || nameLength == 0 || int(nameLength) > MaxAccountNameBytes {
		return nil, ErrAccountEncoding
	}
	name, err := r.take(int(nameLength))
	if err != nil {
		return nil, ErrAccountEncoding
	}
	account := &Account{AccountID: accountID, Name: append([]byte(nil), name...)}
	if account.Kind, err = r.u8(); err != nil {
		return nil, ErrAccountEncoding
	}
	kind, ok := AccountKind(name)
	if !ok || kind != account.Kind {
		return nil, ErrAccountIdentity
	}
	derived, err := DeriveAccountID(name)
	if err != nil || derived != accountID {
		return nil, ErrAccountIdentity
	}
	if account.Balance, err = r.u128(); err != nil {
		return nil, ErrAccountEncoding
	}
	if account.AssetID, err = r.fixed32(); err != nil {
		return nil, ErrAccountEncoding
	}
	if account.HasAsset, err = readBool(r); err != nil {
		return nil, err
	}
	if account.NextSequence, err = r.u64(); err != nil {
		return nil, ErrAccountEncoding
	}
	if account.CreatedAtSequence, err = r.u64(); err != nil {
		return nil, ErrAccountEncoding
	}
	if account.Frozen, err = readBool(r); err != nil {
		return nil, err
	}
	if account.HasOpenReference, err = readBool(r); err != nil {
		return nil, err
	}
	if account.AuthorityKey, err = r.fixed32(); err != nil {
		return nil, ErrAccountEncoding
	}
	if account.HasAuthorityKey, err = readBool(r); err != nil {
		return nil, err
	}
	if r.remaining() != 0 {
		return nil, ErrAccountEncoding
	}
	zero := [32]byte{}
	if (!account.HasAsset && (!account.Balance.IsZero() || account.AssetID != zero)) ||
		(account.HasAsset && account.AssetID == zero) ||
		(!account.HasAuthorityKey && account.AuthorityKey != zero) ||
		(account.HasAuthorityKey && account.AuthorityKey == zero) {
		return nil, ErrAccountEncoding
	}
	return account, nil
}
