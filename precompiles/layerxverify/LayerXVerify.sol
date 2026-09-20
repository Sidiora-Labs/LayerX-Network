// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

address constant LAYERX_VERIFY_PRECOMPILE_ADDRESS = 0x0000000000000000000000000000000000001012;

ILayerXVerify constant LAYERX_VERIFY_CONTRACT = ILayerXVerify(LAYERX_VERIFY_PRECOMPILE_ADDRESS);

/// Stateless verification of LayerX evidence. Every trust anchor is calldata:
/// the precompile reads no chain state, so the caller decides which sequencer
/// key, sequencer identity, batch range, state root or program it trusts.
///
/// verifyEd25519 answers with a boolean. Every other method reverts unless the
/// evidence verifies, so a returned value is a verified fact.
///
/// Gas = 3000 + 16 * len(calldata after the selector)
///     + 4000 * signatures + 100 * proofNodes
/// signatures: 1 for verifyEd25519, verifyReceipt and verifyDiscoveryProof,
/// 2 for verifyReceiptInclusion, 0 for verifyStateProof. proofNodes:
/// len(proof) / 32 for verifyReceiptInclusion, len(witness) / 32 for
/// verifyStateProof.
interface ILayerXVerify {
    struct ReceiptFacts {
        bytes32 receiptDigest;
        bytes32 activityId;
        uint64 globalSequence;
        int32 resultCode;
        uint16 moduleId;
        uint8 operation;
        bytes32 asset;
        uint128 amount;
        bytes32 from;
        bytes32 to;
        bytes32 previousStateRoot;
        bytes32 resultingStateRoot;
        uint64 timestamp;
    }

    struct BatchFacts {
        bytes32 headerDigest;
        uint32 networkId;
        uint64 epoch;
        uint64 batchNumber;
        uint64 firstSequence;
        uint64 lastSequence;
        bytes32 previousStateRoot;
        bytes32 resultingStateRoot;
        bytes32 receiptRoot;
        bytes32 sequencerId;
        uint64 timestampMs;
    }

    struct DiscoveryFacts {
        bytes32 digest;
        uint32 version;
        bytes32 codeHash;
        uint16 abiVersion;
        uint64 observedSequence;
        uint64 observedAt;
        uint64 validThrough;
        bytes32 stateRoot;
        bytes32 headReceiptDigest;
    }

    /// Strict Ed25519 over SHA256(LayerX domain tag || message) for domain
    /// 0..19 (the LayerX hash-domain order), or over message itself for
    /// domain 255. Reverts for any other domain, a signature that is not 64
    /// bytes, or a message above 1 MiB.
    function verifyEd25519(
        bytes32 publicKey,
        uint8 domain,
        bytes calldata message,
        bytes calldata signature
    ) external pure returns (bool valid);

    /// Canonical receipt decode and the sequencer signature over its digest.
    function verifyReceipt(
        bytes calldata receipt,
        bytes32 sequencerPublicKey
    ) external pure returns (ReceiptFacts memory facts);

    /// verifyReceipt plus a Merkle path to the receipt root of a batch header
    /// signed by the named sequencer inside the authorised batch range.
    function verifyReceiptInclusion(
        bytes calldata receipt,
        bytes calldata proof,
        bytes calldata batchHeader,
        bytes calldata headerSignature,
        bytes32 sequencerId,
        bytes32 sequencerPublicKey,
        uint64 firstBatchNumber,
        uint64 lastBatchNumber
    ) external pure returns (ReceiptFacts memory facts, BatchFacts memory batch);

    /// A version-2 native state witness folded to stateRoot.
    function verifyStateProof(
        bytes calldata witness,
        bytes32 stateRoot
    ) external pure returns (uint16 moduleId, bytes memory key, bytes memory value);

    /// A sequencer program head attestation for programId whose validity
    /// window is exactly stalenessMs.
    function verifyDiscoveryProof(
        bytes calldata payload,
        bytes calldata proofMaterial,
        bytes32 programId,
        uint64 stalenessMs,
        bytes32 sequencerPublicKey
    ) external pure returns (DiscoveryFacts memory head);
}
