// SPDX-License-Identifier: LicenseRef-Centra-ai-Protocol
pragma solidity ^0.8.24;

import {CheckpointRegistry} from "../CheckpointRegistry.sol";
import {GuarantorBond} from "../GuarantorBond.sol";
import {Governed} from "../security/Governed.sol";
import {ReentrancyLock} from "../security/ReentrancyLock.sol";
import {LayerXComponent} from "../security/LayerXComponent.sol";
import {Predeploys} from "../deployment/Predeploys.sol";
import {Arithmetic} from "../libraries/Arithmetic.sol";
import {SafeCall} from "../libraries/SafeCall.sol";

contract CheckpointChallengeManager is Governed, ReentrancyLock, LayerXComponent {
    enum Kind {
        Fraud,
        DataAvailability,
        Equivocation
    }
    enum Status {
        None,
        Pending,
        Rejected,
        Upheld
    }

    struct Challenge {
        address challenger;
        bytes32 evidenceHash;
        uint128 bond;
        uint64 raisedAt;
        Kind kind;
        Status status;
    }

    error InvalidChallenge();
    error ChallengeWindowClosed();
    error ChallengePending();
    error TransferFailed();

    CheckpointRegistry public immutable registry;
    GuarantorBond public immutable guarantorBond;
    uint64 public immutable challengePeriod;
    uint128 public immutable minimumChallengeBond;
    mapping(bytes32 => Challenge) public challenge;

    event ChallengeRaised(
        bytes32 indexed checkpointHash, address indexed challenger, Kind kind, bytes32 evidenceHash, uint256 bond
    );
    event ChallengeResolved(bytes32 indexed checkpointHash, bool upheld, uint256 guarantorsSlashed);

    constructor(
        CheckpointRegistry checkpointRegistry,
        GuarantorBond bond,
        address governanceTimelock,
        address emergencyAuthority,
        uint64 period,
        uint128 challengeBond,
        bytes32 componentConfigHash,
        uint192 componentRelease
    )
        Governed(governanceTimelock, emergencyAuthority)
        LayerXComponent(Predeploys.CHALLENGE_MANAGER, componentConfigHash, componentRelease)
    {
        if (
            address(checkpointRegistry) == address(0) || address(bond) == address(0) || period < 1 hours
                || challengeBond == 0
        ) revert InvalidChallenge();
        registry = checkpointRegistry;
        guarantorBond = bond;
        challengePeriod = period;
        minimumChallengeBond = challengeBond;
    }

    function windowClosesAt(bytes32 checkpointHash) public view returns (uint64) {
        uint64 registered = registry.registeredAt(checkpointHash);
        if (registered == 0) revert InvalidChallenge();
        return Arithmetic.toUint64(uint256(registered) + challengePeriod);
    }

    function claimable(bytes32 checkpointHash) external view returns (bool) {
        Challenge storage item = challenge[checkpointHash];
        return registry.isCanonicalCheckpoint(checkpointHash) && block.timestamp >= windowClosesAt(checkpointHash)
            && item.status != Status.Pending
            && item.status != Status.Upheld;
    }

    function raiseChallenge(bytes32 checkpointHash, Kind kind, bytes32 evidenceHash) external payable nonReentrant {
        if (
            checkpointHash == bytes32(0) || evidenceHash == bytes32(0) || msg.value < minimumChallengeBond
                || block.timestamp > windowClosesAt(checkpointHash) || challenge[checkpointHash].status != Status.None
        ) {
            revert ChallengeWindowClosed();
        }
        challenge[checkpointHash] = Challenge({
            challenger: msg.sender,
            evidenceHash: evidenceHash,
            bond: Arithmetic.toUint128(msg.value),
            raisedAt: Arithmetic.toUint64(block.timestamp),
            kind: kind,
            status: Status.Pending
        });
        emit ChallengeRaised(checkpointHash, msg.sender, kind, evidenceHash, msg.value);
    }

    function resolveChallenge(bytes32 checkpointHash, bool upheld) external onlyGovernance nonReentrant {
        Challenge storage item = challenge[checkpointHash];
        if (item.status != Status.Pending) revert InvalidChallenge();
        item.status = upheld ? Status.Upheld : Status.Rejected;
        bytes32[] memory guarantors = registry.guarantorIds(checkpointHash);
        if (upheld) {
            registry.invalidateCheckpoint(checkpointHash);
            uint64 removedEpoch = Arithmetic.toUint64(uint256(registry.checkpointEpoch(checkpointHash)) + 1);
            for (uint256 i = 0; i < guarantors.length; ++i) {
                guarantorBond.slashForCheckpoint(guarantors[i], checkpointHash, removedEpoch);
            }
        }
        address recipient = upheld ? item.challenger : governance;
        uint256 amount = item.bond;
        item.bond = 0;
        SafeCall.CallResult memory result = SafeCall.sendValue(recipient, amount, 100_000);
        if (!result.success) revert TransferFailed();
        emit ChallengeResolved(checkpointHash, upheld, upheld ? guarantors.length : 0);
    }
}
