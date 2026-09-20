package codec

import "crypto/sha256"

// Domain is one hashing purpose declared by lxp_domain_tag_id, in core order.
type Domain uint8

const (
	DomainActivityID Domain = iota
	DomainPayloadHash
	DomainSignaturePreimage
	DomainAuthorityHash
	DomainContextHash
	DomainMerkleLeaf
	DomainMerkleInternal
	DomainBatchHeader
	DomainReceipt
	DomainCheckpointCertificate
	DomainAccountID
	DomainDidID
	DomainEvmPayoutBinding
	DomainStateLeaf
	DomainStateNode
	DomainStateRootChain
	DomainSnapshot
	DomainDaChunk
	DomainDaChallenge
	DomainGuarantorAttestation
	domainCount
)

var domainTags = [domainCount]string{
	"LXP/v1/activity-id\x00",
	"LXP/v1/payload-hash\x00",
	"LXP/v1/signature-preimage\x00",
	"LXP/v1/authority-hash\x00",
	"LXP/v1/context-hash\x00",
	"LXP/v1/merkle-leaf\x00",
	"LXP/v1/merkle-internal\x00",
	"LXP/v1/batch-header\x00",
	"LXP/v1/receipt\x00",
	"LXP/v2/checkpoint-certificate\x00",
	"LXP/v1/account-id\x00",
	"LXP/v1/did-id\x00",
	"LXP/v1/evm-payout-binding\x00",
	"LXP/v1/state-leaf\x00",
	"LXP/v1/state-node\x00",
	"LXP/v1/state-root-chain\x00",
	"LXP/v1/snapshot\x00",
	"LXP/v1/da-chunk\x00",
	"LXP/v1/da-challenge\x00",
	"LXP/v2/guarantor-attestation\x00",
}

// ProgramDiscoveryProofDomain prefixes the program discovery proof digest.
const ProgramDiscoveryProofDomain = "LayerX/program-discovery-proof/v1\x00"

// Tag returns the exact NUL-terminated core domain tag.
func (d Domain) Tag() ([]byte, error) {
	if d >= domainCount {
		return nil, ErrInvalidTag
	}
	return []byte(domainTags[d]), nil
}

// DomainHash computes SHA256(tag || canonical) exactly as lxp_hash_domain.
func DomainHash(d Domain, canonical []byte) ([32]byte, error) {
	if d >= domainCount {
		return [32]byte{}, ErrInvalidTag
	}
	h := sha256.New()
	h.Write([]byte(domainTags[d]))
	h.Write(canonical)
	var out [32]byte
	h.Sum(out[:0])
	return out, nil
}

func mustDomainHash(d Domain, parts ...[]byte) [32]byte {
	h := sha256.New()
	h.Write([]byte(domainTags[d]))
	for _, part := range parts {
		h.Write(part)
	}
	var out [32]byte
	h.Sum(out[:0])
	return out
}

// ReceiptDigest hashes an unsigned canonical receipt under the receipt domain.
func ReceiptDigest(unsignedReceipt []byte) [32]byte {
	return mustDomainHash(DomainReceipt, unsignedReceipt)
}

// BatchHeaderDigest hashes the exact canonical batch header.
func BatchHeaderDigest(header []byte) [32]byte {
	return mustDomainHash(DomainBatchHeader, header)
}

// MerkleLeafHash hashes canonical leaf bytes under the batch Merkle leaf domain.
func MerkleLeafHash(leaf []byte) [32]byte { return mustDomainHash(DomainMerkleLeaf, leaf) }

// MerkleNodeHash hashes two children under the batch Merkle internal domain.
func MerkleNodeHash(left, right [32]byte) [32]byte {
	return mustDomainHash(DomainMerkleInternal, left[:], right[:])
}
