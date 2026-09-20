package verify

import (
	"errors"

	"github.com/sidiora-labs/paxeer-network/layerxproof/codec"
)

var (
	ErrReceiptProtocolVersion  = errors.New("layerx verify: receipt protocol version")
	ErrReceiptActivityID       = errors.New("layerx verify: receipt activity identifier")
	ErrReceiptMissingSignature = errors.New("layerx verify: receipt signature absent")
	ErrSequencerSignature      = errors.New("layerx verify: sequencer signature")

	ErrHeaderCanonical     = errors.New("layerx verify: batch header canonical form")
	ErrHeaderBatchNumber   = errors.New("layerx verify: batch outside authorised range")
	ErrSequencerIdentity   = errors.New("layerx verify: sequencer identity")
	ErrHeaderSignature     = errors.New("layerx verify: batch header signature")
	ErrReceiptBatchBinding = errors.New("layerx verify: receipt outside batch sequence range")
)

// SequencerAuthorization is the sequencer authority valid for an inclusive
// batch range. It is the trust anchor of every header-bound proof.
type SequencerAuthorization struct {
	SequencerID      [32]byte
	PublicKey        [32]byte
	FirstBatchNumber uint64
	LastBatchNumber  uint64
}

// VerifiedReceipt is a receipt whose canonical form and sequencer signature
// passed under the pinned sequencer key.
type VerifiedReceipt struct {
	Receipt *codec.Receipt
	Digest  [32]byte
}

// VerifiedBatchHeader is a canonical header whose authority range, sequencer
// identity and signature passed.
type VerifiedBatchHeader struct {
	Header *codec.BatchHeader
	Digest [32]byte
}

// ReceiptSignature is the receipt-authenticity primitive
// (layerx-proof receipt::verify_sequencer_signature): canonical decode,
// supported protocol, non-zero activity, present signature, and a strict
// Ed25519 signature over the receipt digest under sequencerPublicKey.
func ReceiptSignature(receiptBytes []byte, sequencerPublicKey [32]byte) (*VerifiedReceipt, error) {
	receipt, err := codec.DecodeReceipt(receiptBytes)
	if err != nil {
		return nil, err
	}
	if !codec.ProtocolVersionUsesOccupancy(receipt.ProtocolVersion) {
		return nil, ErrReceiptProtocolVersion
	}
	if receipt.ActivityID == ([32]byte{}) {
		return nil, ErrReceiptActivityID
	}
	if receipt.SequencerSignature == nil {
		return nil, ErrReceiptMissingSignature
	}
	digest := receipt.Digest()
	if err := Ed25519Digest(sequencerPublicKey, *receipt.SequencerSignature, digest); err != nil {
		return nil, ErrSequencerSignature
	}
	return &VerifiedReceipt{Receipt: receipt, Digest: digest}, nil
}

// BatchHeader verifies a canonical batch header under sequencer authority
// (layerx-proof inclusion::verify_header).
func BatchHeader(headerBytes []byte, signature [64]byte, authorization SequencerAuthorization) (*VerifiedBatchHeader, error) {
	header, err := codec.DecodeBatchHeader(headerBytes)
	if err != nil {
		return nil, err
	}
	if len(headerBytes) != codec.BatchHeaderBytes || !codec.ProtocolVersionUsesOccupancy(header.ProtocolVersion) {
		return nil, ErrHeaderCanonical
	}
	if header.BatchNumber < authorization.FirstBatchNumber || header.BatchNumber > authorization.LastBatchNumber {
		return nil, ErrHeaderBatchNumber
	}
	if header.SequencerID != authorization.SequencerID {
		return nil, ErrSequencerIdentity
	}
	digest := codec.BatchHeaderDigest(headerBytes)
	if err := Ed25519Digest(authorization.PublicKey, signature, digest); err != nil {
		return nil, ErrHeaderSignature
	}
	return &VerifiedBatchHeader{Header: header, Digest: digest}, nil
}

// ReceiptAtRoot proves canonical receipt bytes included under a trusted
// receipt Merkle root and authenticates the receipt under the sequencer key.
func ReceiptAtRoot(receiptBytes []byte, proof *codec.MerkleProof, receiptRoot [32]byte, sequencerPublicKey [32]byte) (*VerifiedReceipt, error) {
	verified, err := ReceiptSignature(receiptBytes, sequencerPublicKey)
	if err != nil {
		return nil, err
	}
	if err := codec.VerifyMerklePath(receiptBytes, proof, receiptRoot); err != nil {
		return nil, err
	}
	return verified, nil
}

// ReceiptInclusion proves a receipt included in a sequencer-signed batch
// header: header authority and signature, the Merkle path to the header's
// receipt root, the receipt's own sequencer signature, and the receipt's
// protocol version and global sequence inside the header's committed range
// (the binding lxp_receipt_verify_checkpointed applies).
func ReceiptInclusion(receiptBytes []byte, proof *codec.MerkleProof, headerBytes []byte, headerSignature [64]byte,
	authorization SequencerAuthorization) (*VerifiedReceipt, *VerifiedBatchHeader, error) {
	header, err := BatchHeader(headerBytes, headerSignature, authorization)
	if err != nil {
		return nil, nil, err
	}
	receipt, err := ReceiptAtRoot(receiptBytes, proof, header.Header.ReceiptMerkleRoot, authorization.PublicKey)
	if err != nil {
		return nil, nil, err
	}
	if receipt.Receipt.ProtocolVersion != header.Header.ProtocolVersion ||
		receipt.Receipt.GlobalSequence < header.Header.FirstSequence ||
		receipt.Receipt.GlobalSequence > header.Header.LastSequence {
		return nil, nil, ErrReceiptBatchBinding
	}
	return receipt, header, nil
}
