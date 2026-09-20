package codec

import "errors"

const (
	proofTag uint16 = 0x4d50
	// MaxProofDepth is the maximum Merkle path depth of every LayerX tree.
	MaxProofDepth = 32
	// MaxMerkleProofBytes is the largest canonical wire Merkle proof.
	MaxMerkleProofBytes = 4 + 4 + 4 + 1 + 4 + MaxProofDepth*32

	evidenceProofVersion     uint8 = 1
	evidenceProofPrefixBytes       = 10
)

var (
	ErrMerkleEmptyTree        = errors.New("layerx merkle: empty tree")
	ErrMerkleLeafIndex        = errors.New("layerx merkle: leaf index outside tree")
	ErrMerklePathLength       = errors.New("layerx merkle: path length")
	ErrMerklePromotionSibling = errors.New("layerx merkle: odd-node promotion sibling")
	ErrMerkleRootMismatch     = errors.New("layerx merkle: root mismatch")
)

// MerkleProof is one index-aware Merkle path.
type MerkleProof struct {
	LeafIndex uint32
	LeafCount uint32
	Siblings  [][32]byte
}

// ProofDepth returns the unique path depth implied by a leaf count.
func ProofDepth(count uint32) int {
	depth := 0
	for count > 1 {
		count = count/2 + count%2
		depth++
	}
	return depth
}

// Validate applies the structural rules of layerx-proof merkle::Proof::new.
func (p *MerkleProof) Validate() error {
	if p.LeafCount == 0 {
		return ErrMerkleEmptyTree
	}
	if p.LeafIndex >= p.LeafCount {
		return ErrMerkleLeafIndex
	}
	if len(p.Siblings) > MaxProofDepth || len(p.Siblings) != ProofDepth(p.LeafCount) {
		return ErrMerklePathLength
	}
	return nil
}

// DecodeMerkleProof decodes the canonical wire Merkle proof (tag 0x4d50)
// written by lxp_merkle_proof_encode and layerx-wire encode_merkle_proof.
func DecodeMerkleProof(bytes []byte) (*MerkleProof, error) {
	if len(bytes) > MaxMerkleProofBytes {
		return nil, ErrLengthLimit
	}
	r := &reader{bytes: bytes}
	if err := r.structureHeader(proofTag); err != nil {
		return nil, err
	}
	index, err := r.u32()
	if err != nil {
		return nil, err
	}
	count, err := r.u32()
	if err != nil {
		return nil, err
	}
	depth, err := r.u8()
	if err != nil {
		return nil, err
	}
	if count == 0 || index >= count || int(depth) > MaxProofDepth || int(depth) != ProofDepth(count) {
		return nil, ErrNonCanonical
	}
	siblings, err := r.prefixed(MaxProofDepth * 32)
	if err != nil {
		return nil, err
	}
	if len(siblings) != int(depth)*32 {
		return nil, ErrNonCanonical
	}
	if err := r.finish(); err != nil {
		return nil, err
	}
	proof := &MerkleProof{LeafIndex: index, LeafCount: count, Siblings: make([][32]byte, depth)}
	for level := range proof.Siblings {
		copy(proof.Siblings[level][:], siblings[level*32:])
	}
	return proof, nil
}

// DecodeEvidenceProof decodes the version-1 public evidence exchange form of
// layerx-proof merkle::decode_proof.
func DecodeEvidenceProof(bytes []byte) (*MerkleProof, error) {
	if len(bytes) < evidenceProofPrefixBytes || bytes[0] != evidenceProofVersion {
		return nil, ErrNonCanonical
	}
	r := &reader{bytes: bytes[1:]}
	index, _ := r.u32()
	count, _ := r.u32()
	depth, _ := r.u8()
	if int(depth) > MaxProofDepth || r.remaining() != int(depth)*32 {
		return nil, ErrNonCanonical
	}
	proof := &MerkleProof{LeafIndex: index, LeafCount: count, Siblings: make([][32]byte, depth)}
	for level := range proof.Siblings {
		proof.Siblings[level], _ = r.fixed32()
	}
	if err := proof.Validate(); err != nil {
		return nil, err
	}
	return proof, nil
}

func foldPath(current [32]byte, proof *MerkleProof, node func(l, r [32]byte) [32]byte) ([32]byte, error) {
	if err := proof.Validate(); err != nil {
		return [32]byte{}, err
	}
	index, count := proof.LeafIndex, proof.LeafCount
	for _, sibling := range proof.Siblings {
		if (index^1) >= count && sibling != current {
			return [32]byte{}, ErrMerklePromotionSibling
		}
		if index&1 == 0 {
			current = node(current, sibling)
		} else {
			current = node(sibling, current)
		}
		index /= 2
		count = count/2 + count%2
	}
	return current, nil
}

// VerifyMerklePath recomputes the batch Merkle root from canonical leaf bytes.
func VerifyMerklePath(leaf []byte, proof *MerkleProof, expectedRoot [32]byte) error {
	return VerifyMerkleLeafHash(MerkleLeafHash(leaf), proof, expectedRoot)
}

// VerifyMerkleLeafHash verifies a path from an already domain-separated leaf.
func VerifyMerkleLeafHash(leafHash [32]byte, proof *MerkleProof, expectedRoot [32]byte) error {
	root, err := foldPath(leafHash, proof, MerkleNodeHash)
	if err != nil {
		return err
	}
	if root != expectedRoot {
		return ErrMerkleRootMismatch
	}
	return nil
}
