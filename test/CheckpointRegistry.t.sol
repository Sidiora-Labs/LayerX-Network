// SPDX-License-Identifier: LicenseRef-Centra-ai-Protocol
pragma solidity ^0.8.24;

import {CanonicalCheckpoint} from "../contracts/libraries/CanonicalCheckpoint.sol";
import {CheckpointRegistry} from "../contracts/CheckpointRegistry.sol";
import {GuarantorBond} from "../contracts/GuarantorBond.sol";

interface CheckpointVm {
    function addr(uint256 privateKey) external returns (address);
    function deal(address account, uint256 balance) external;
    function prank(address sender) external;
    function sign(uint256 privateKey, bytes32 digest) external returns (uint8 v, bytes32 r, bytes32 s);
    function warp(uint256 timestamp) external;
    function expectRevert(bytes4 selector) external;
}

contract CheckpointRegistryTest {
    CheckpointVm private constant vm = CheckpointVm(address(uint160(uint256(keccak256("hevm cheat code")))));

    GuarantorBond private bond;
    CheckpointRegistry private registry;
    uint256[3] private keys = [uint256(1), uint256(2), uint256(3)];
    bytes32 private constant GENESIS = bytes32(uint256(0x11) << 248);
    bytes32 private constant CONFIG = keccak256("checkpoint-test-config");
    uint192 private constant RELEASE = uint192(1) << 128;

    function setUp() public {
        vm.warp(1000);
        bond = new GuarantorBond(address(this), address(this), 100, 200 ether, 7 days, CONFIG, RELEASE);
        for (uint256 i = 0; i < keys.length; ++i) {
            address signer = vm.addr(keys[i]);
            bond.activateGuarantor(bytes32(i + 1), signer, signer, 1, uint64(i + 1));
            vm.deal(signer, 3 ether);
            vm.prank(signer);
            bond.depositBond{value: 2 ether}(bytes32(i + 1));
        }
        registry = new CheckpointRegistry(bond, 1, 42, 2, 4, 1 hours, 5 minutes, GENESIS, CONFIG, RELEASE);
    }

    function testCanonicalHeaderMatchesCVector() public view {
        CanonicalCheckpoint.HeaderCommitments memory header = _header();
        bytes memory encoded = registry.canonicalHeader(header);
        require(encoded.length == 354, "header length");
        require(
            registry.checkpointHash(header, "") == 0xf655c001cc9392bddb71932afa21742e7be6ac762e76f3ce0c56e32e8ec35aee,
            "C checkpoint vector mismatch"
        );
    }

    function testRegistersThresholdCertificate() public {
        CanonicalCheckpoint.HeaderCommitments memory header = _header();
        bytes32 digest = registry.checkpointHash(header, "");
        CanonicalCheckpoint.GuarantorAttestation[] memory attestations = _attestations(header, digest, 2);
        registry.registerCheckpoint(header, "", attestations);
        require(registry.latestFinalisedStateRoot() == header.resultingStateRoot, "root not advanced");
        require(registry.checkpointAtBatch(1) == digest, "batch checkpoint absent");
        require(registry.checkpointGuarantorSetVersion(digest) == 3, "guarantor-set version absent");
    }

    function testRejectsSignatureOutsideGovernedMembership() public {
        CanonicalCheckpoint.HeaderCommitments memory header = _header();
        bytes32 digest = registry.checkpointHash(header, "");
        CanonicalCheckpoint.GuarantorAttestation[] memory attestations = _attestations(header, digest, 2);
        attestations[1].guarantorId = bytes32(uint256(4));
        attestations[1].signer = vm.addr(4);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(4, CanonicalCheckpoint.attestationHash(attestations[1]));
        attestations[1].v = v;
        attestations[1].r = r;
        attestations[1].s = s;
        vm.expectRevert(CheckpointRegistry.InvalidCertificate.selector);
        registry.registerCheckpoint(header, "", attestations);
    }

    function testRejectsDuplicateGuarantor() public {
        CanonicalCheckpoint.HeaderCommitments memory header = _header();
        bytes32 digest = registry.checkpointHash(header, "");
        CanonicalCheckpoint.GuarantorAttestation[] memory attestations = _attestations(header, digest, 2);
        attestations[1] = attestations[0];
        vm.expectRevert(CheckpointRegistry.InvalidCertificate.selector);
        registry.registerCheckpoint(header, "", attestations);
    }

    function testRejectsStateRootDiscontinuity() public {
        CanonicalCheckpoint.HeaderCommitments memory header = _header();
        header.previousStateRoot = keccak256("unrecorded-state-root");
        bytes32 digest = registry.checkpointHash(header, "");
        CanonicalCheckpoint.GuarantorAttestation[] memory attestations = _attestations(header, digest, 2);
        vm.expectRevert(CheckpointRegistry.StateRootDiscontinuity.selector);
        registry.registerCheckpoint(header, "", attestations);
    }

    function testRegisteredCertificateRequiresExactRecordedSet() public {
        CanonicalCheckpoint.HeaderCommitments memory header = _header();
        bytes32 digest = registry.checkpointHash(header, "");
        CanonicalCheckpoint.GuarantorAttestation[] memory attestations = _attestations(header, digest, 2);
        registry.registerCheckpoint(header, "", attestations);

        require(
            registry.verifyRegisteredCertificate(
                digest,
                header.resultingStateRoot,
                header.epoch,
                header.batchNumber,
                header.dataAvailabilityRoot,
                attestations
            ),
            "recorded certificate rejected"
        );
        require(
            !registry.verifyRegisteredCertificate(
                digest,
                header.resultingStateRoot,
                header.epoch + 1,
                header.batchNumber,
                header.dataAvailabilityRoot,
                attestations
            ),
            "caller-selected membership epoch accepted"
        );

        attestations[0].attestedAt += 1;
        require(
            !registry.verifyRegisteredCertificate(
                digest,
                header.resultingStateRoot,
                header.epoch,
                header.batchNumber,
                header.dataAvailabilityRoot,
                attestations
            ),
            "mutated certificate accepted"
        );
    }

    function testOnlySlashingAuthorityCanInvalidateWithoutErasingHistory() public {
        CanonicalCheckpoint.HeaderCommitments memory header = _header();
        bytes32 digest = registry.checkpointHash(header, "");
        CanonicalCheckpoint.GuarantorAttestation[] memory attestations = _attestations(header, digest, 2);
        registry.registerCheckpoint(header, "", attestations);

        vm.expectRevert(CheckpointRegistry.ChallengeAuthorityOnly.selector);
        vm.prank(address(0xBEEF));
        registry.invalidateCheckpoint(digest);

        bond.setSlashingAuthority(address(this));
        registry.invalidateCheckpoint(digest);
        require(registry.explicitlyInvalidated(digest), "invalidation not recorded");
        require(!registry.isCanonicalCheckpoint(digest), "invalid checkpoint remained canonical");
        require(registry.finalisedStateRoot(digest) == header.resultingStateRoot, "state-root history erased");
        require(registry.checkpointAtBatch(header.batchNumber) == digest, "batch history erased");
        require(registry.latestCanonicalCheckpointHash() == bytes32(0), "nonexistent predecessor invented");

        header.epoch = 2;
        header.batchNumber = 2;
        header.firstSequence = header.lastSequence + 1;
        header.lastSequence = header.firstSequence;
        header.previousStateRoot = header.resultingStateRoot;
        header.resultingStateRoot = bytes32(uint256(0x99) << 248);
        header.timestamp += 1;
        bytes32 descendantDigest = registry.checkpointHash(header, "");
        CanonicalCheckpoint.GuarantorAttestation[] memory descendantAttestations =
            _attestations(header, descendantDigest, 2);
        vm.expectRevert(CheckpointRegistry.CanonicalChainInvalidated.selector);
        registry.registerCheckpoint(header, "", descendantAttestations);
    }

    function testPayloadSizeIndependentOfActivityCount() public view {
        CanonicalCheckpoint.HeaderCommitments memory first = _header();
        CanonicalCheckpoint.HeaderCommitments memory second = first;
        second.lastSequence = 10_000_000;
        require(
            registry.canonicalHeader(first).length == registry.canonicalHeader(second).length, "count leaked into size"
        );
    }

    function _header() private pure returns (CanonicalCheckpoint.HeaderCommitments memory header) {
        header = CanonicalCheckpoint.HeaderCommitments({
            protocolVersion: 1,
            networkId: 42,
            epoch: 1,
            batchNumber: 1,
            firstSequence: 1,
            lastSequence: 1_000_000,
            previousStateRoot: GENESIS,
            resultingStateRoot: bytes32(uint256(0x22) << 248),
            activityMerkleRoot: bytes32(uint256(0x33) << 248),
            receiptMerkleRoot: bytes32(uint256(0x44) << 248),
            eventMerkleRoot: bytes32(uint256(0x55) << 248),
            dataAvailabilityRoot: bytes32(uint256(0x66) << 248),
            oracleRoot: bytes32(uint256(0x77) << 248),
            timestamp: 1000,
            sequencerId: bytes32(uint256(0x88) << 248)
        });
    }

    function _attestations(CanonicalCheckpoint.HeaderCommitments memory header, bytes32 digest, uint256 count)
        private
        returns (CanonicalCheckpoint.GuarantorAttestation[] memory attestations)
    {
        attestations = new CanonicalCheckpoint.GuarantorAttestation[](count);
        for (uint256 i = 0; i < count; ++i) {
            attestations[i] = CanonicalCheckpoint.GuarantorAttestation({
                checkpointId: digest,
                checkpointHash: digest,
                guarantorId: bytes32(i + 1),
                batchNumber: header.batchNumber,
                dataAvailabilityRoot: header.dataAvailabilityRoot,
                replayed: true,
                dataAvailable: true,
                availabilityClassMask: 0x1f,
                attestedAt: header.timestamp + 1,
                signer: vm.addr(keys[i]),
                r: bytes32(0),
                s: bytes32(0),
                v: 0
            });
            bytes32 attestationDigest = CanonicalCheckpoint.attestationHash(attestations[i]);
            (uint8 v, bytes32 r, bytes32 s) = vm.sign(keys[i], attestationDigest);
            attestations[i].v = v;
            attestations[i].r = r;
            attestations[i].s = s;
        }
    }
}
