package types

import (
	"encoding/binary"
	"math/big"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/crypto"
)

// The attestation layout is documented byte for byte in ATTESTATION.md and
// pinned by testdata/preimage-vectors.json, which the sidecar and the kernel
// assert too.
const (
	Domain = "PAXEERX_WEB_V1"

	// OriginEVM is a request made through the xweb precompile; OriginProgram
	// is a request made by a kernel program.
	OriginEVM     uint8 = 1
	OriginProgram uint8 = 2

	PreimageLength = 14 + 1 + 32 + 32 + 8 + 1 + 32 + 32 + 32 + 4

	// SignatureLength is a secp256k1 signature r || s || v.
	SignatureLength = 65
)

// Attestation is what attestors sign for one request's answer. NetworkID is
// the EVM chain id for origin 1 and the kernel network id for origin 2; it
// must be set and fit in 256 bits.
type Attestation struct {
	Origin        uint8
	NetworkID     *big.Int
	Requester     Hash32
	RequestID     uint64
	Kind          uint8
	PayloadHash   Hash32
	ContentDigest Hash32
	ResponseHash  Hash32
	FullLength    uint32
}

// EVMRequester is an EVM address left-padded with zeros to 32 bytes.
func EVMRequester(address Address20) Hash32 {
	var out Hash32
	copy(out[12:], address[:])
	return out
}

// Keccak is keccak256 of data.
func Keccak(data []byte) Hash32 { return Hash32(crypto.Keccak256Hash(data)) }

// Preimage is "PAXEERX_WEB_V1" || uint8 origin || uint256 networkId ||
// bytes32 requester || uint64 requestId || uint8 kind || bytes32
// keccak256(payload) || bytes32 contentDigest || bytes32 keccak256(response)
// || uint32 fullLength, integers big-endian: 188 bytes.
func Preimage(a Attestation) []byte {
	out := make([]byte, 0, PreimageLength)
	out = append(out, Domain...)
	out = append(out, a.Origin)
	out = append(out, common.LeftPadBytes(a.NetworkID.Bytes(), 32)...)
	out = append(out, a.Requester[:]...)
	out = append(out, u64(a.RequestID)...)
	out = append(out, a.Kind)
	out = append(out, a.PayloadHash[:]...)
	out = append(out, a.ContentDigest[:]...)
	out = append(out, a.ResponseHash[:]...)
	length := make([]byte, 4)
	binary.BigEndian.PutUint32(length, a.FullLength)
	out = append(out, length...)
	return out
}

// Digest is keccak256(Preimage): what attestors sign.
func Digest(a Attestation) Hash32 { return Keccak(Preimage(a)) }
