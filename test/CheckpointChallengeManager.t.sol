// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {CanonicalCheckpoint} from "../contracts/libraries/CanonicalCheckpoint.sol";
import {CheckpointRegistry} from "../contracts/CheckpointRegistry.sol";
import {CheckpointChallengeManager} from "../contracts/challenge/CheckpointChallengeManager.sol";
import {GuarantorBond} from "../contracts/GuarantorBond.sol";
import {Constants} from "../contracts/libraries/Constants.sol";
import {Governed} from "../contracts/security/Governed.sol";
import {CheckpointToken, CheckpointVm} from "./CheckpointRegistry.t.sol";

contract RejectingChallenger {
    CheckpointChallengeManager private immutable manager;

    constructor(CheckpointChallengeManager target) {
        manager = target;
    }

    function raise(bytes32 checkpointHash, CheckpointChallengeManager.Kind kind, bytes32 evidenceHash)
        external
        payable
    {
        manager.raiseChallenge{value: msg.value}(checkpointHash, kind, evidenceHash);
    }

    function withdraw() external {
        manager.withdrawBond();
    }
}

contract ReenteringChallenger {
    CheckpointChallengeManager private immutable manager;
    uint256 public received;
    uint256 public reentered;

    constructor(CheckpointChallengeManager target) {
        manager = target;
    }

    function raise(bytes32 checkpointHash, CheckpointChallengeManager.Kind kind, bytes32 evidenceHash)
        external
        payable
    {
        manager.raiseChallenge{value: msg.value}(checkpointHash, kind, evidenceHash);
    }

    function withdraw() external {
        manager.withdrawBond();
    }

    receive() external payable {
        received += msg.value;
        (bool success,) = address(manager).call(abi.encodeCall(CheckpointChallengeManager.withdrawBond, ()));
        if (success) reentered += 1;
    }
}

contract CheckpointChallengeManagerTest {
    CheckpointVm private constant vm = CheckpointVm(address(uint160(uint256(keccak256("hevm cheat code")))));

    GuarantorBond private bond;
    CheckpointRegistry private registry;
    CheckpointChallengeManager private manager;
    uint256[3] private keys = [uint256(1), uint256(2), uint256(3)];
    bytes32 private constant GENESIS_RECEIPT_ROOT = bytes32(uint256(0x11) << 248);
    bytes32 private constant GENESIS_CANONICAL_STATE_ROOT = bytes32(uint256(0x12) << 248);
    bytes32 private constant GENESIS_MANIFEST_DIGEST = bytes32(uint256(0x13) << 248);
    bytes32 private constant CONFIG = keccak256("challenge-test-config");
    uint192 private constant RELEASE = uint192(1) << 128;
    address private constant EMERGENCY_COUNCIL = address(0xEC01);
    uint64 private constant CHALLENGE_PERIOD = 7 days;
    uint128 private constant MINIMUM_BOND = 1 ether;

    receive() external payable {}

    function setUp() public {
        vm.warp(1000);
        vm.deal(address(this), 10 ether);
        CheckpointToken implementation = new CheckpointToken();
        vm.etch(Constants.USDL_TOKEN, address(implementation).code);
        bond = new GuarantorBond(
            address(this),
            address(this),
            Constants.USDL_TOKEN,
            address(this),
            Constants.USDL_ASSET_ID,
            Constants.PROTOCOL_VERSION,
            42,
            100,
            7 days,
            CONFIG,
            RELEASE
        );
        CheckpointToken token = CheckpointToken(Constants.USDL_TOKEN);
        for (uint256 i = 0; i < keys.length; ++i) {
            address signer = vm.addr(keys[i]);
            bond.activateGuarantor(bytes32(i + 1), signer, signer, 1, uint64(i + 1));
            token.mint(signer, 2 ether);
            vm.prank(signer);
            token.approve(address(bond), 2 ether);
            vm.prank(signer);
            bond.depositBond(bytes32(i + 1), 2 ether);
        }
        registry = new CheckpointRegistry(
            bond,
            Constants.PROTOCOL_VERSION,
            42,
            2,
            4,
            5 minutes,
            5 minutes,
            GENESIS_MANIFEST_DIGEST,
            GENESIS_CANONICAL_STATE_ROOT,
            GENESIS_RECEIPT_ROOT,
            CONFIG,
            RELEASE
        );
        manager = new CheckpointChallengeManager(
            registry, bond, address(this), EMERGENCY_COUNCIL, CHALLENGE_PERIOD, MINIMUM_BOND, CONFIG, RELEASE
        );
        bond.setSlashingAuthority(address(manager));
    }

    function testUpheldResolutionSucceedsWhenChallengerRejectsValue() public {
        bytes32 checkpointHash = _registerCheckpoint();
        RejectingChallenger challenger = new RejectingChallenger(manager);
        challenger.raise{value: 1 ether}(
            checkpointHash, CheckpointChallengeManager.Kind.Fraud, keccak256("invalid-transition")
        );

        manager.resolveChallenge(checkpointHash, true);

        (,, uint128 recordedBond,,, CheckpointChallengeManager.Status status) = manager.challenge(checkpointHash);
        require(status == CheckpointChallengeManager.Status.Upheld, "challenge not upheld");
        require(recordedBond == 0, "bond not cleared from challenge record");
        require(registry.explicitlyInvalidated(checkpointHash), "checkpoint not invalidated");
        require(!registry.isCanonicalCheckpoint(checkpointHash), "upheld checkpoint remained canonical");
        require(bond.bondRecord(bytes32(uint256(1))).jailed, "first guarantor not slashed");
        require(bond.bondRecord(bytes32(uint256(2))).jailed, "second guarantor not slashed");
        require(manager.owedBond(address(challenger)) == 1 ether, "bond not owed to rejecting challenger");
        require(address(manager).balance == 1 ether, "bond left custody during resolution");

        vm.expectRevert(CheckpointChallengeManager.TransferFailed.selector);
        challenger.withdraw();
        require(manager.owedBond(address(challenger)) == 1 ether, "failed withdrawal lost the owed bond");
        require(address(manager).balance == 1 ether, "failed withdrawal moved value");

        vm.expectRevert(CheckpointChallengeManager.InvalidChallenge.selector);
        manager.resolveChallenge(checkpointHash, true);
    }

    function testWithdrawBondPaysOwedAmountExactlyOnce() public {
        bytes32 checkpointHash = _registerCheckpoint();
        address challenger = address(0xC0FFEE);
        vm.deal(challenger, 3 ether);
        vm.prank(challenger);
        manager.raiseChallenge{value: 1.5 ether}(
            checkpointHash, CheckpointChallengeManager.Kind.DataAvailability, keccak256("missing-shard")
        );
        require(challenger.balance == 1.5 ether, "bond not escrowed");

        vm.recordLogs();
        manager.resolveChallenge(checkpointHash, true);
        CheckpointVm.Log[] memory logs = vm.getRecordedLogs();
        _requireBondOwedEvent(logs, checkpointHash, challenger, 1.5 ether);
        _requireResolvedEvent(logs, checkpointHash, true, 2);
        require(challenger.balance == 1.5 ether, "bond pushed during resolution");
        require(manager.owedBond(challenger) == 1.5 ether, "owed bond not recorded");

        vm.recordLogs();
        vm.prank(challenger);
        manager.withdrawBond();
        _requireBondWithdrawnEvent(vm.getRecordedLogs(), challenger, 1.5 ether);
        require(challenger.balance == 3 ether, "owed bond not paid exactly");
        require(manager.owedBond(challenger) == 0, "owed bond not cleared");
        require(address(manager).balance == 0, "value retained after withdrawal");

        vm.expectRevert(CheckpointChallengeManager.NoBondOwed.selector);
        vm.prank(challenger);
        manager.withdrawBond();
        require(challenger.balance == 3 ether, "second withdrawal changed balance");
    }

    function testWithdrawBondWithoutOwedBondReverts() public {
        vm.expectRevert(CheckpointChallengeManager.NoBondOwed.selector);
        vm.prank(address(0xBEEF));
        manager.withdrawBond();
    }

    function testRejectedResolutionOwesBondToGovernanceForPull() public {
        bytes32 checkpointHash = _registerCheckpoint();
        address challenger = address(0xC0FFEE);
        vm.deal(challenger, 1 ether);
        vm.prank(challenger);
        manager.raiseChallenge{value: 1 ether}(
            checkpointHash, CheckpointChallengeManager.Kind.Equivocation, keccak256("double-sign")
        );
        uint256 governanceBalance = address(this).balance;

        vm.expectRevert(Governed.GovernanceOnly.selector);
        vm.prank(challenger);
        manager.resolveChallenge(checkpointHash, false);

        manager.resolveChallenge(checkpointHash, false);
        (,,,,, CheckpointChallengeManager.Status status) = manager.challenge(checkpointHash);
        require(status == CheckpointChallengeManager.Status.Rejected, "challenge not rejected");
        require(registry.isCanonicalCheckpoint(checkpointHash), "rejected challenge invalidated checkpoint");
        require(!bond.bondRecord(bytes32(uint256(1))).jailed, "rejected challenge slashed guarantor");
        require(address(this).balance == governanceBalance, "forfeited bond pushed during resolution");
        require(manager.owedBond(address(this)) == 1 ether, "forfeited bond not owed to governance");
        require(manager.owedBond(challenger) == 0, "rejected challenger owed a refund");

        vm.expectRevert(CheckpointChallengeManager.NoBondOwed.selector);
        vm.prank(challenger);
        manager.withdrawBond();

        manager.withdrawBond();
        require(address(this).balance == governanceBalance + 1 ether, "forfeited bond not paid to governance");
        require(manager.owedBond(address(this)) == 0, "forfeited bond still owed");
        vm.warp(manager.windowClosesAt(checkpointHash));
        require(manager.claimable(checkpointHash), "rejected checkpoint not claimable");
    }

    function testWithdrawBondBlocksReentrantDoubleClaim() public {
        bytes32 checkpointHash = _registerCheckpoint();
        ReenteringChallenger challenger = new ReenteringChallenger(manager);
        challenger.raise{value: 1 ether}(
            checkpointHash, CheckpointChallengeManager.Kind.Fraud, keccak256("invalid-transition")
        );
        manager.resolveChallenge(checkpointHash, true);
        require(manager.owedBond(address(challenger)) == 1 ether, "bond not owed to reentering challenger");

        challenger.withdraw();
        require(challenger.reentered() == 0, "reentrant withdrawal succeeded");
        require(challenger.received() == 1 ether, "reentering challenger not paid exactly once");
        require(address(challenger).balance == 1 ether, "reentering challenger not holding exactly the bond");
        require(manager.owedBond(address(challenger)) == 0, "owed bond not cleared");
        require(address(manager).balance == 0, "value retained after reentrant attempt");

        vm.expectRevert(CheckpointChallengeManager.NoBondOwed.selector);
        challenger.withdraw();
    }

    function _registerCheckpoint() private returns (bytes32 digest) {
        CanonicalCheckpoint.HeaderCommitments memory header = _header();
        digest = registry.checkpointHash(header, "");
        registry.registerCheckpoint(header, "", _attestations(header, digest, 2));
        require(registry.registeredAt(digest) == block.timestamp, "checkpoint not registered");
    }

    function _requireBondOwedEvent(
        CheckpointVm.Log[] memory logs,
        bytes32 checkpointHash,
        address recipient,
        uint256 amount
    ) private view {
        bytes32 signature = keccak256("BondOwed(bytes32,address,uint256)");
        for (uint256 i = 0; i < logs.length; ++i) {
            if (logs[i].emitter != address(manager) || logs[i].topics.length != 3 || logs[i].topics[0] != signature) {
                continue;
            }
            require(logs[i].topics[1] == checkpointHash, "BondOwed checkpoint");
            require(logs[i].topics[2] == bytes32(uint256(uint160(recipient))), "BondOwed recipient");
            require(keccak256(logs[i].data) == keccak256(abi.encode(amount)), "BondOwed amount");
            return;
        }
        revert("BondOwed event absent");
    }

    function _requireResolvedEvent(
        CheckpointVm.Log[] memory logs,
        bytes32 checkpointHash,
        bool upheld,
        uint256 guarantorsSlashed
    ) private view {
        bytes32 signature = keccak256("ChallengeResolved(bytes32,bool,uint256)");
        for (uint256 i = 0; i < logs.length; ++i) {
            if (logs[i].emitter != address(manager) || logs[i].topics.length != 2 || logs[i].topics[0] != signature) {
                continue;
            }
            require(logs[i].topics[1] == checkpointHash, "ChallengeResolved checkpoint");
            require(
                keccak256(logs[i].data) == keccak256(abi.encode(upheld, guarantorsSlashed)), "ChallengeResolved data"
            );
            return;
        }
        revert("ChallengeResolved event absent");
    }

    function _requireBondWithdrawnEvent(CheckpointVm.Log[] memory logs, address recipient, uint256 amount)
        private
        view
    {
        bytes32 signature = keccak256("BondWithdrawn(address,uint256)");
        for (uint256 i = 0; i < logs.length; ++i) {
            if (logs[i].emitter != address(manager) || logs[i].topics.length != 2 || logs[i].topics[0] != signature) {
                continue;
            }
            require(logs[i].topics[1] == bytes32(uint256(uint160(recipient))), "BondWithdrawn recipient");
            require(keccak256(logs[i].data) == keccak256(abi.encode(amount)), "BondWithdrawn amount");
            return;
        }
        revert("BondWithdrawn event absent");
    }

    function _header() private pure returns (CanonicalCheckpoint.HeaderCommitments memory header) {
        header = CanonicalCheckpoint.HeaderCommitments({
            protocolVersion: Constants.PROTOCOL_VERSION,
            networkId: 42,
            epoch: 1,
            batchNumber: 1,
            firstSequence: 1,
            lastSequence: 1_000_000,
            previousStateRoot: GENESIS_RECEIPT_ROOT,
            resultingStateRoot: bytes32(uint256(0x22) << 248),
            activityMerkleRoot: bytes32(uint256(0x33) << 248),
            receiptMerkleRoot: bytes32(uint256(0x44) << 248),
            eventMerkleRoot: bytes32(uint256(0x55) << 248),
            dataAvailabilityRoot: bytes32(uint256(0x66) << 248),
            oracleRoot: bytes32(uint256(0x77) << 248),
            timestamp: 1_000_000,
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
                protocolVersion: header.protocolVersion,
                networkId: header.networkId,
                paxeerChainId: uint64(block.chainid),
                settlementContract: address(bond),
                epoch: header.epoch,
                checkpointId: digest,
                checkpointHash: digest,
                guarantorId: bytes32(i + 1),
                batchNumber: header.batchNumber,
                dataAvailabilityRoot: header.dataAvailabilityRoot,
                replayed: true,
                dataAvailable: true,
                availabilityClassMask: Constants.ALL_AVAILABILITY_CLASSES,
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
