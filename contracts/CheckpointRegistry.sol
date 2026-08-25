// SPDX-License-Identifier: LicenseRef-Centra-ai-Protocol
pragma solidity ^0.8.24;

import {CanonicalCheckpoint} from "./libraries/CanonicalCheckpoint.sol";
import {Constants} from "./libraries/Constants.sol";
import {CryptographyPrimitives} from "./libraries/CryptographyPrimitives.sol";
import {Arithmetic} from "./libraries/Arithmetic.sol";
import {IGuarantorEligibility} from "./interfaces/IGuarantorEligibility.sol";
import {Predeploys} from "./deployment/Predeploys.sol";
import {LayerXComponent} from "./security/LayerXComponent.sol";

contract CheckpointRegistry is LayerXComponent {
    error InvalidConfiguration();
    error InvalidHeader();
    error InvalidCertificate();
    error StateRootDiscontinuity();
    error ChallengeAuthorityOnly();
    error CanonicalChainInvalidated();

    IGuarantorEligibility public immutable guarantorEligibility;
    uint16 public immutable protocolVersion;
    uint32 public immutable networkId;
    uint16 public immutable threshold;
    uint16 public immutable maximumAttestations;
    uint64 public immutable maximumAttestationDelay;
    uint64 public immutable maximumFutureTimestampDrift;

    mapping(bytes32 => bytes32) public finalisedStateRoot;
    mapping(uint64 => bytes32) public checkpointAtBatch;
    mapping(bytes32 => uint64) public registeredAt;
    mapping(bytes32 => uint64) public checkpointEpoch;
    mapping(bytes32 => uint64) public checkpointBatchNumber;
    mapping(bytes32 => uint64) public checkpointGuarantorSetVersion;
    mapping(bytes32 => uint64) public checkpointTimestamp;
    mapping(bytes32 => bytes32) public certificateCommitment;
    mapping(bytes32 => bool) public explicitlyInvalidated;
    mapping(bytes32 => bytes32[]) private checkpointGuarantors;
    uint64 public firstInvalidatedBatch;
    uint64 public finalisedEpoch;
    uint64 public finalisedBatchNumber;
    uint64 public finalisedLastSequence;
    uint64 public finalisedTimestamp;
    bytes32 public latestFinalisedStateRoot;

    event CheckpointRegistered(
        bytes32 indexed checkpointHash,
        uint64 indexed epoch,
        uint64 indexed batchNumber,
        uint64 firstSequence,
        uint64 lastSequence,
        bytes32 previousStateRoot,
        bytes32 resultingStateRoot,
        bytes32 dataAvailabilityRoot,
        uint64 guarantorSetVersion
    );
    event CheckpointInvalidated(
        bytes32 indexed checkpointHash,
        uint64 indexed invalidatedBatch,
        bytes32 indexed lastCanonicalCheckpoint,
        uint64 lastCanonicalBatch
    );

    constructor(
        IGuarantorEligibility eligibility,
        uint16 expectedProtocolVersion,
        uint32 expectedNetworkId,
        uint16 certificateThreshold,
        uint16 attestationLimit,
        uint64 attestationDelay,
        uint64 futureTimestampDrift,
        bytes32 genesisStateRoot,
        bytes32 componentConfigHash,
        uint192 componentRelease
    ) LayerXComponent(Predeploys.CHECKPOINT_REGISTRY, componentConfigHash, componentRelease) {
        if (
            address(eligibility) == address(0) || expectedProtocolVersion == 0 || expectedNetworkId == 0
                || certificateThreshold == 0 || attestationLimit < certificateThreshold || attestationLimit > 32
                || attestationDelay == 0 || futureTimestampDrift == 0 || genesisStateRoot == bytes32(0)
        ) {
            revert InvalidConfiguration();
        }
        guarantorEligibility = eligibility;
        protocolVersion = expectedProtocolVersion;
        networkId = expectedNetworkId;
        threshold = certificateThreshold;
        maximumAttestations = attestationLimit;
        maximumAttestationDelay = attestationDelay;
        maximumFutureTimestampDrift = futureTimestampDrift;
        latestFinalisedStateRoot = genesisStateRoot;
    }

    function canonicalHeader(CanonicalCheckpoint.HeaderCommitments calldata header)
        external
        pure
        returns (bytes memory)
    {
        return CanonicalCheckpoint.encodeHeader(header);
    }

    function checkpointHash(CanonicalCheckpoint.HeaderCommitments calldata header, bytes calldata validityProof)
        public
        pure
        returns (bytes32)
    {
        return CanonicalCheckpoint.checkpointHash(header, validityProof);
    }

    function registerCheckpoint(
        CanonicalCheckpoint.HeaderCommitments calldata header,
        bytes calldata validityProof,
        CanonicalCheckpoint.GuarantorAttestation[] calldata attestations
    ) external returns (bytes32 digest) {
        _validateHeader(header);
        if (attestations.length < threshold || attestations.length > maximumAttestations) {
            revert InvalidCertificate();
        }
        digest = CanonicalCheckpoint.checkpointHash(header, validityProof);
        bytes32 previousGuarantorId;
        uint256 valid;
        for (uint256 i = 0; i < attestations.length; ++i) {
            CanonicalCheckpoint.GuarantorAttestation calldata attestation = attestations[i];
            if (
                attestation.guarantorId <= previousGuarantorId || attestation.checkpointId != digest
                    || attestation.checkpointHash != digest || attestation.batchNumber != header.batchNumber
                    || attestation.dataAvailabilityRoot != header.dataAvailabilityRoot || !attestation.replayed
                    || !attestation.dataAvailable
                    || attestation.availabilityClassMask != Constants.ALL_AVAILABILITY_CLASSES
                    || attestation.attestedAt < header.timestamp
                    || uint256(attestation.attestedAt) > uint256(header.timestamp) + maximumAttestationDelay
                    || !_validSignature(attestation)
                    || !guarantorEligibility.bondedActive(attestation.guarantorId, attestation.signer, header.epoch)
            ) revert InvalidCertificate();
            previousGuarantorId = attestation.guarantorId;
            ++valid;
        }
        if (valid < threshold) revert InvalidCertificate();
        finalisedStateRoot[digest] = header.resultingStateRoot;
        checkpointAtBatch[header.batchNumber] = digest;
        registeredAt[digest] = Arithmetic.toUint64(block.timestamp);
        checkpointEpoch[digest] = header.epoch;
        checkpointBatchNumber[digest] = header.batchNumber;
        checkpointGuarantorSetVersion[digest] = guarantorEligibility.membershipVersion();
        checkpointTimestamp[digest] = header.timestamp;
        certificateCommitment[digest] = sha256(abi.encode(attestations));
        for (uint256 i = 0; i < attestations.length; ++i) {
            checkpointGuarantors[digest].push(attestations[i].guarantorId);
        }
        finalisedEpoch = header.epoch;
        finalisedBatchNumber = header.batchNumber;
        finalisedLastSequence = header.lastSequence;
        finalisedTimestamp = header.timestamp;
        latestFinalisedStateRoot = header.resultingStateRoot;
        emit CheckpointRegistered(
            digest,
            header.epoch,
            header.batchNumber,
            header.firstSequence,
            header.lastSequence,
            header.previousStateRoot,
            header.resultingStateRoot,
            header.dataAvailabilityRoot,
            checkpointGuarantorSetVersion[digest]
        );
    }

    function isFinalised(bytes32 digest, bytes32 stateRoot) external view returns (bool) {
        return stateRoot != bytes32(0) && finalisedStateRoot[digest] == stateRoot && isCanonicalCheckpoint(digest);
    }

    function isCanonicalCheckpoint(bytes32 digest) public view returns (bool) {
        uint64 batchNumber = checkpointBatchNumber[digest];
        return batchNumber != 0 && checkpointAtBatch[batchNumber] == digest
            && (firstInvalidatedBatch == 0 || batchNumber < firstInvalidatedBatch);
    }

    function latestCanonicalCheckpointHash() public view returns (bytes32) {
        uint64 invalidatedBatch = firstInvalidatedBatch;
        if (invalidatedBatch == 0) return checkpointAtBatch[finalisedBatchNumber];
        if (invalidatedBatch == 1) return bytes32(0);
        return checkpointAtBatch[invalidatedBatch - 1];
    }

    function invalidateCheckpoint(bytes32 digest) external {
        if (msg.sender != guarantorEligibility.slashingAuthority()) revert ChallengeAuthorityOnly();
        uint64 batchNumber = checkpointBatchNumber[digest];
        if (batchNumber == 0 || checkpointAtBatch[batchNumber] != digest) revert InvalidHeader();
        explicitlyInvalidated[digest] = true;
        uint64 invalidatedBatch = firstInvalidatedBatch;
        if (invalidatedBatch == 0 || batchNumber < invalidatedBatch) {
            firstInvalidatedBatch = batchNumber;
            invalidatedBatch = batchNumber;
        }
        uint64 lastCanonicalBatch = invalidatedBatch - 1;
        emit CheckpointInvalidated(
            digest,
            batchNumber,
            lastCanonicalBatch == 0 ? bytes32(0) : checkpointAtBatch[lastCanonicalBatch],
            lastCanonicalBatch
        );
    }

    function guarantorIds(bytes32 digest) external view returns (bytes32[] memory) {
        return checkpointGuarantors[digest];
    }

    function verifyRegisteredCertificate(
        bytes32 digest,
        bytes32 stateRoot,
        uint64 epoch,
        uint64 batchNumber,
        bytes32 dataAvailabilityRoot,
        CanonicalCheckpoint.GuarantorAttestation[] calldata attestations
    ) external view returns (bool) {
        if (
            stateRoot == bytes32(0) || finalisedStateRoot[digest] != stateRoot
                || !isCanonicalCheckpoint(digest)
                || checkpointAtBatch[batchNumber] != digest || checkpointEpoch[digest] != epoch
                || attestations.length < threshold
                || attestations.length > maximumAttestations
                || certificateCommitment[digest] != sha256(abi.encode(attestations))
        ) {
            return false;
        }
        bytes32 previousGuarantorId;
        uint64 timestamp = checkpointTimestamp[digest];
        for (uint256 i = 0; i < attestations.length; ++i) {
            CanonicalCheckpoint.GuarantorAttestation calldata attestation = attestations[i];
            if (
                attestation.guarantorId <= previousGuarantorId || attestation.checkpointId != digest
                    || attestation.checkpointHash != digest || attestation.batchNumber != batchNumber
                    || attestation.dataAvailabilityRoot != dataAvailabilityRoot || !attestation.replayed
                    || !attestation.dataAvailable
                    || attestation.availabilityClassMask != Constants.ALL_AVAILABILITY_CLASSES
                    || attestation.attestedAt < timestamp
                    || uint256(attestation.attestedAt) > uint256(timestamp) + maximumAttestationDelay
                    || !_validSignature(attestation)
                    || !guarantorEligibility.bondedActive(attestation.guarantorId, attestation.signer, epoch)
            ) {
                return false;
            }
            previousGuarantorId = attestation.guarantorId;
        }
        return true;
    }

    function isRecordedCertificate(bytes32 digest, CanonicalCheckpoint.GuarantorAttestation[] calldata attestations)
        external
        view
        returns (bool)
    {
        return
            finalisedStateRoot[digest] != bytes32(0)
                && certificateCommitment[digest] == sha256(abi.encode(attestations));
    }

    function _validateHeader(CanonicalCheckpoint.HeaderCommitments calldata header) private view {
        if (firstInvalidatedBatch != 0) revert CanonicalChainInvalidated();
        if (
            header.protocolVersion != protocolVersion || header.networkId != networkId || header.epoch <= finalisedEpoch
                || header.batchNumber != finalisedBatchNumber + 1 || header.firstSequence != finalisedLastSequence + 1
                || header.lastSequence < header.firstSequence || header.timestamp <= finalisedTimestamp
                || header.timestamp > block.timestamp + maximumFutureTimestampDrift
                || header.resultingStateRoot == bytes32(0) || header.activityMerkleRoot == bytes32(0)
                || header.receiptMerkleRoot == bytes32(0) || header.eventMerkleRoot == bytes32(0)
                || header.dataAvailabilityRoot == bytes32(0) || header.oracleRoot == bytes32(0)
                || header.sequencerId == bytes32(0)
        ) revert InvalidHeader();
        if (header.previousStateRoot != latestFinalisedStateRoot) {
            revert StateRootDiscontinuity();
        }
    }

    function _validSignature(CanonicalCheckpoint.GuarantorAttestation calldata attestation)
        private
        pure
        returns (bool)
    {
        if (attestation.signer == address(0)) return false;
        return CryptographyPrimitives.recoverOrZero(
            CanonicalCheckpoint.attestationHash(attestation), attestation.r, attestation.s, attestation.v
        ) == attestation.signer;
    }
}
