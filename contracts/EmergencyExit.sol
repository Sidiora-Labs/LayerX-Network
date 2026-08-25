// SPDX-License-Identifier: LicenseRef-Centra-ai-Protocol
pragma solidity ^0.8.24;

import {CanonicalCheckpoint} from "./libraries/CanonicalCheckpoint.sol";
import {PaxeerWithdrawalCodec} from "./libraries/PaxeerWithdrawalCodec.sol";
import {CheckpointRegistry} from "./CheckpointRegistry.sol";
import {LayerXVault} from "./custody/LayerXVault.sol";
import {CheckpointChallengeManager} from "./challenge/CheckpointChallengeManager.sol";
import {WithdrawalNullifierRegistry} from "./storage/WithdrawalNullifierRegistry.sol";
import {Governed} from "./security/Governed.sol";
import {ReentrancyLock} from "./security/ReentrancyLock.sol";
import {LayerXComponent} from "./security/LayerXComponent.sol";
import {Predeploys} from "./deployment/Predeploys.sol";

contract EmergencyExit is Governed, ReentrancyLock, LayerXComponent {
    struct ExitClaim {
        bytes32 withdrawalId;
        bytes32 account;
        bytes32 assetId;
        uint128 finalisedBalance;
        address recipient;
        bytes32 checkpointHash;
    }

    struct BalanceProof {
        uint256 leafIndex;
        bytes32[] siblings;
    }

    error ExitNotEligible();
    error InvalidExitClaim();
    error ExitAlreadyConsumed();

    CheckpointRegistry public immutable registry;
    CheckpointChallengeManager public immutable challengeManager;
    WithdrawalNullifierRegistry public immutable nullifierRegistry;
    LayerXVault public immutable vault;
    uint64 public immutable livenessBound;
    uint32 public immutable networkId;
    bool public governanceEmergency;

    event EmergencyDeclarationSet(bool enabled);
    event EmergencyExitExecuted(
        bytes32 indexed claimId,
        bytes32 indexed nullifier,
        bytes32 indexed checkpointHash,
        bytes32 account,
        bytes32 assetId,
        address recipient,
        uint256 amount
    );

    constructor(
        CheckpointRegistry checkpointRegistry,
        CheckpointChallengeManager checkpointChallengeManager,
        WithdrawalNullifierRegistry withdrawalNullifiers,
        LayerXVault custodyVault,
        address governanceTimelock,
        address emergencyAuthority,
        uint64 checkpointLivenessBound,
        bytes32 componentConfigHash,
        uint192 componentRelease
    )
        Governed(governanceTimelock, emergencyAuthority)
        LayerXComponent(Predeploys.EMERGENCY_EXIT, componentConfigHash, componentRelease)
    {
        if (
            address(checkpointRegistry) == address(0) || address(checkpointChallengeManager) == address(0)
                || address(withdrawalNullifiers) == address(0) || address(custodyVault) == address(0)
                || checkpointLivenessBound < 1 hours
        ) revert InvalidExitClaim();
        registry = checkpointRegistry;
        challengeManager = checkpointChallengeManager;
        nullifierRegistry = withdrawalNullifiers;
        vault = custodyVault;
        livenessBound = checkpointLivenessBound;
        networkId = checkpointRegistry.networkId();
    }

    function declareEmergency(bool enabled) external onlyGovernance {
        governanceEmergency = enabled;
        emit EmergencyDeclarationSet(enabled);
    }

    function emergencyCouncilDeclare() external onlyEmergencyCouncil {
        governanceEmergency = true;
        emit EmergencyDeclarationSet(true);
    }

    function latestCheckpointHash() public view returns (bytes32) {
        return registry.latestCanonicalCheckpointHash();
    }

    function eligible() public view returns (bool) {
        bytes32 checkpointHash = latestCheckpointHash();
        if (checkpointHash == bytes32(0)) return false;
        return governanceEmergency || registry.firstInvalidatedBatch() != 0
            || block.timestamp >= uint256(registry.registeredAt(checkpointHash)) + livenessBound;
    }

    function requiredWithdrawalId(bytes32 account, bytes32 assetId, bytes32 checkpointHash)
        public
        view
        returns (bytes32)
    {
        return
            sha256(abi.encodePacked("LXP/v1/emergency-withdrawal-id\x00", networkId, account, assetId, checkpointHash));
    }

    function exitNullifier(ExitClaim calldata exitClaim) public view returns (bytes32) {
        return PaxeerWithdrawalCodec.nullifier(
            networkId,
            exitClaim.withdrawalId,
            exitClaim.account,
            exitClaim.assetId,
            exitClaim.finalisedBalance,
            exitClaim.checkpointHash
        );
    }

    function executeExit(
        ExitClaim calldata exitClaim,
        bytes32 stateRoot,
        BalanceProof calldata balanceProof,
        CanonicalCheckpoint.GuarantorAttestation[] calldata recordedAttestations
    ) external nonReentrant returns (bytes32 claimId) {
        bytes32 nullifier = exitNullifier(exitClaim);
        if (
            nullifierRegistry.status(nullifier) != WithdrawalNullifierRegistry.Status.None
                || nullifierRegistry.withdrawalIdUsed(exitClaim.withdrawalId)
        ) {
            revert ExitAlreadyConsumed();
        }
        if (!eligible()) revert ExitNotEligible();
        bytes32 latest = latestCheckpointHash();
        if (
            exitClaim.checkpointHash != latest
                || exitClaim.withdrawalId != requiredWithdrawalId(exitClaim.account, exitClaim.assetId, latest)
                || exitClaim.account == bytes32(0) || exitClaim.assetId == bytes32(0) || exitClaim.finalisedBalance == 0
                || exitClaim.recipient == address(0) || !registry.isFinalised(latest, stateRoot)
                || !registry.isRecordedCertificate(latest, recordedAttestations)
                || PaxeerWithdrawalCodec.proofRoot(
                        PaxeerWithdrawalCodec.balanceLeaf(
                            exitClaim.account, exitClaim.assetId, exitClaim.finalisedBalance, exitClaim.recipient
                        ),
                        balanceProof.leafIndex,
                        balanceProof.siblings
                    ) != stateRoot
        ) {
            revert InvalidExitClaim();
        }
        claimId = sha256(abi.encode("LXP/Paxeer/emergency-exit/v1", block.chainid, address(this), nullifier));
        nullifierRegistry.reserve(nullifier, exitClaim.withdrawalId, claimId);
        nullifierRegistry.consume(nullifier, claimId);
        vault.release(claimId, exitClaim.assetId, exitClaim.recipient, exitClaim.finalisedBalance);
        emit EmergencyExitExecuted(
            claimId,
            nullifier,
            latest,
            exitClaim.account,
            exitClaim.assetId,
            exitClaim.recipient,
            exitClaim.finalisedBalance
        );
    }
}
