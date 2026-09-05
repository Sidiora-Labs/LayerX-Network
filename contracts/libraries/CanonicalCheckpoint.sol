// SPDX-License-Identifier: LicenseRef-Centra-ai-Protocol
pragma solidity ^0.8.24;

import {Arithmetic} from "./Arithmetic.sol";
import {Constants} from "./Constants.sol";

library CanonicalCheckpoint {
    error ValidityProofTooLarge(uint256 length);
    bytes internal constant CHECKPOINT_DOMAIN = "LXP/v2/checkpoint-certificate\x00";
    bytes internal constant ATTESTATION_DOMAIN = "LXP/v2/guarantor-attestation\x00";
    uint256 internal constant HEADER_LENGTH = 354;

    struct HeaderCommitments {
        uint16 protocolVersion;
        uint32 networkId;
        uint64 epoch;
        uint64 batchNumber;
        uint64 firstSequence;
        uint64 lastSequence;
        bytes32 previousStateRoot;
        bytes32 resultingStateRoot;
        bytes32 activityMerkleRoot;
        bytes32 receiptMerkleRoot;
        bytes32 eventMerkleRoot;
        bytes32 dataAvailabilityRoot;
        bytes32 oracleRoot;
        uint64 timestamp;
        bytes32 sequencerId;
    }

    struct GuarantorAttestation {
        uint16 protocolVersion;
        uint32 networkId;
        uint64 paxeerChainId;
        address settlementContract;
        uint64 epoch;
        bytes32 checkpointId;
        bytes32 checkpointHash;
        bytes32 guarantorId;
        uint64 batchNumber;
        bytes32 dataAvailabilityRoot;
        bool replayed;
        bool dataAvailable;
        uint8 availabilityClassMask;
        uint64 attestedAt;
        address signer;
        bytes32 r;
        bytes32 s;
        uint8 v;
    }

    function encodeHeader(HeaderCommitments calldata header) internal pure returns (bytes memory encoded) {
        encoded = bytes.concat(
            header.protocolVersion == 3 ? bytes5(hex"000317010f") : bytes5(hex"000217010f"),
            abi.encodePacked(
                uint8(1),
                header.protocolVersion,
                uint8(2),
                header.networkId,
                uint8(3),
                header.epoch,
                uint8(4),
                header.batchNumber,
                uint8(5),
                header.firstSequence,
                uint8(6),
                header.lastSequence
            ),
            _hashField(7, header.previousStateRoot),
            _hashField(8, header.resultingStateRoot),
            _hashField(9, header.activityMerkleRoot),
            _hashField(10, header.receiptMerkleRoot),
            _hashField(11, header.eventMerkleRoot),
            _hashField(12, header.dataAvailabilityRoot),
            _hashField(13, header.oracleRoot),
            abi.encodePacked(uint8(14), header.timestamp),
            _hashField(15, header.sequencerId)
        );
        assert(encoded.length == HEADER_LENGTH);
    }

    function checkpointHash(HeaderCommitments calldata header, bytes calldata validityProof)
        internal
        pure
        returns (bytes32)
    {
        if (validityProof.length > Constants.MAX_CANONICAL_BYTES) {
            revert ValidityProofTooLarge(validityProof.length);
        }
        return sha256(
            bytes.concat(
                CHECKPOINT_DOMAIN,
                encodeHeader(header),
                abi.encodePacked(Arithmetic.toUint32(validityProof.length)),
                validityProof
            )
        );
    }

    function attestationHash(GuarantorAttestation memory attestation) internal pure returns (bytes32) {
        return sha256(
            bytes.concat(
                ATTESTATION_DOMAIN,
                abi.encodePacked(
                    attestation.protocolVersion,
                    attestation.networkId,
                    attestation.paxeerChainId,
                    attestation.settlementContract,
                    attestation.epoch,
                    attestation.checkpointId,
                    attestation.checkpointHash,
                    attestation.guarantorId,
                    attestation.batchNumber,
                    attestation.dataAvailabilityRoot,
                    uint8(attestation.replayed ? 1 : 0),
                    uint8(attestation.dataAvailable ? 1 : 0),
                    attestation.availabilityClassMask,
                    attestation.attestedAt
                )
            )
        );
    }

    function _hashField(uint8 fieldId, bytes32 value) private pure returns (bytes memory) {
        return abi.encodePacked(fieldId, uint32(32), value);
    }
}
