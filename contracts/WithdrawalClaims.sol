// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {NativeStateProof} from "./libraries/NativeStateProof.sol";
import {CanonicalCheckpoint} from "./libraries/CanonicalCheckpoint.sol";
import {CheckpointRegistry} from "./CheckpointRegistry.sol";
import {LayerXVault} from "./custody/LayerXVault.sol";
import {CheckpointChallengeManager} from "./challenge/CheckpointChallengeManager.sol";
import {WithdrawalNullifierRegistry} from "./storage/WithdrawalNullifierRegistry.sol";
import {ReentrancyLock} from "./security/ReentrancyLock.sol";
import {PaxeerWithdrawalCodec} from "./libraries/PaxeerWithdrawalCodec.sol";
import {Arithmetic} from "./libraries/Arithmetic.sol";
import {LayerXComponent} from "./security/LayerXComponent.sol";
import {Predeploys} from "./deployment/Predeploys.sol";

contract WithdrawalClaims is ReentrancyLock, LayerXComponent {
    enum ClaimStatus {
        None,
        Pending,
        Paid,
        Cancelled
    }

    struct Withdrawal {
        bytes32 withdrawalId;
        bytes32 account;
        bytes32 assetId;
        uint128 amount;
        address recipient;
        bytes32 checkpointHash;
    }

    struct StateProof {
        uint256 leafIndex;
        bytes32[] siblings;
    }

    struct Claim {
        bytes32 nullifier;
        bytes32 checkpointHash;
        bytes32 assetId;
        address recipient;
        uint128 amount;
        uint64 availableAt;
        ClaimStatus status;
    }

    error InvalidClaim();
    error NullifierAlreadyUsed();
    error ClaimNotReady();
    error ChallengedCheckpoint();

    CheckpointRegistry public immutable registry;
    CheckpointChallengeManager public immutable challengeManager;
    WithdrawalNullifierRegistry public immutable nullifierRegistry;
    LayerXVault public immutable vault;
    uint32 public immutable networkId;
    mapping(bytes32 => Claim) public claim;
    mapping(bytes32 => uint256) public pendingAmount;

    event ClaimQueued(
        bytes32 indexed claimId,
        bytes32 indexed nullifier,
        bytes32 indexed checkpointHash,
        bytes32 assetId,
        address recipient,
        uint256 amount,
        uint64 availableAt
    );
    event ClaimFinalised(bytes32 indexed claimId, bytes32 indexed nullifier);
    event ClaimCancelled(bytes32 indexed claimId, bytes32 indexed nullifier);

    constructor(
        CheckpointRegistry checkpointRegistry,
        CheckpointChallengeManager checkpointChallengeManager,
        WithdrawalNullifierRegistry withdrawalNullifiers,
        LayerXVault custodyVault,
        bytes32 componentConfigHash,
        uint192 componentRelease
    ) LayerXComponent(Predeploys.WITHDRAWAL_CLAIMS, componentConfigHash, componentRelease) {
        if (
            address(checkpointRegistry) == address(0) || address(checkpointChallengeManager) == address(0)
                || address(withdrawalNullifiers) == address(0) || address(custodyVault) == address(0)
        ) revert InvalidClaim();
        registry = checkpointRegistry;
        challengeManager = checkpointChallengeManager;
        nullifierRegistry = withdrawalNullifiers;
        vault = custodyVault;
        networkId = checkpointRegistry.networkId();
    }

    function withdrawalNullifier(Withdrawal calldata withdrawal) public view returns (bytes32) {
        return PaxeerWithdrawalCodec.nullifier(
            networkId,
            withdrawal.withdrawalId,
            withdrawal.account,
            withdrawal.assetId,
            withdrawal.amount,
            withdrawal.checkpointHash
        );
    }

    function withdrawalLeaf(Withdrawal calldata withdrawal) public pure returns (bytes32) {
        return PaxeerWithdrawalCodec.withdrawalLeaf(
            withdrawal.withdrawalId, withdrawal.account, withdrawal.assetId, withdrawal.amount, withdrawal.recipient
        );
    }

    function queueClaim(
        Withdrawal calldata withdrawal,
        bytes32 stateRoot,
        uint64 epoch,
        uint64 batchNumber,
        bytes32 dataAvailabilityRoot,
        StateProof calldata stateProof,
        CanonicalCheckpoint.GuarantorAttestation[] calldata attestations
    ) external nonReentrant returns (bytes32 claimId) {
        bytes32 nullifier = withdrawalNullifier(withdrawal);
        if (nullifierRegistry.status(nullifier) != WithdrawalNullifierRegistry.Status.None) {
            revert NullifierAlreadyUsed();
        }
        if (
            withdrawal.withdrawalId == bytes32(0) || withdrawal.account == bytes32(0)
                || withdrawal.assetId == bytes32(0) || withdrawal.amount == 0 || withdrawal.recipient == address(0)
                || !registry.isFinalised(withdrawal.checkpointHash, stateRoot)
                || !registry.verifyRegisteredCertificate(
                    withdrawal.checkpointHash, stateRoot, epoch, batchNumber, dataAvailabilityRoot, attestations
                ) || !_verifyProof(withdrawalLeaf(withdrawal), stateProof, stateRoot)
        ) {
            revert InvalidClaim();
        }
        uint64 availableAt = challengeManager.windowClosesAt(withdrawal.checkpointHash);
        claimId = sha256(
            abi.encode("LXP/Paxeer/withdrawal-claim/v1", block.chainid, address(this), nullifier, withdrawal.recipient)
        );
        if (claim[claimId].status != ClaimStatus.None) revert InvalidClaim();
        claim[claimId] = Claim({
            nullifier: nullifier,
            checkpointHash: withdrawal.checkpointHash,
            assetId: withdrawal.assetId,
            recipient: withdrawal.recipient,
            amount: withdrawal.amount,
            availableAt: availableAt,
            status: ClaimStatus.Pending
        });
        pendingAmount[withdrawal.assetId] = Arithmetic.add(pendingAmount[withdrawal.assetId], withdrawal.amount);
        nullifierRegistry.reserve(nullifier, withdrawal.withdrawalId, claimId);
        emit ClaimQueued(
            claimId,
            nullifier,
            withdrawal.checkpointHash,
            withdrawal.assetId,
            withdrawal.recipient,
            withdrawal.amount,
            availableAt
        );
    }

    function queueClaim(
        Withdrawal calldata withdrawal,
        bytes32 inclusionCheckpoint,
        bytes32 stateRoot,
        uint64 epoch,
        uint64 batchNumber,
        bytes32 dataAvailabilityRoot,
        bytes calldata witness,
        CanonicalCheckpoint.GuarantorAttestation[] calldata attestations
    ) external nonReentrant returns (bytes32 claimId) {
        bytes32 nullifier = withdrawalNullifier(withdrawal);
        if (nullifierRegistry.status(nullifier) != WithdrawalNullifierRegistry.Status.None) {
            revert NullifierAlreadyUsed();
        }
        if (
            withdrawal.withdrawalId == bytes32(0) || withdrawal.account == bytes32(0)
                || withdrawal.assetId == bytes32(0) || withdrawal.amount == 0 || withdrawal.recipient == address(0)
                || !registry.isRecordedAncestor(withdrawal.checkpointHash, inclusionCheckpoint)
                || !registry.isFinalised(inclusionCheckpoint, stateRoot)
                || !registry.verifyRegisteredCertificate(
                    inclusionCheckpoint, stateRoot, epoch, batchNumber, dataAvailabilityRoot, attestations
                )
        ) revert InvalidClaim();
        NativeStateProof.verifyWithdrawal(
            witness,
            stateRoot,
            networkId,
            withdrawal.withdrawalId,
            withdrawal.account,
            withdrawal.assetId,
            withdrawal.amount,
            withdrawal.recipient,
            withdrawal.checkpointHash,
            nullifier
        );
        uint64 availableAt = challengeManager.windowClosesAt(inclusionCheckpoint);
        claimId = sha256(
            abi.encode("LXP/Paxeer/withdrawal-claim/v1", block.chainid, address(this), nullifier, withdrawal.recipient)
        );
        if (claim[claimId].status != ClaimStatus.None) revert InvalidClaim();
        claim[claimId] = Claim(
            nullifier,
            inclusionCheckpoint,
            withdrawal.assetId,
            withdrawal.recipient,
            withdrawal.amount,
            availableAt,
            ClaimStatus.Pending
        );
        pendingAmount[withdrawal.assetId] = Arithmetic.add(pendingAmount[withdrawal.assetId], withdrawal.amount);
        nullifierRegistry.reserve(nullifier, withdrawal.withdrawalId, claimId);
        emit ClaimQueued(
            claimId,
            nullifier,
            inclusionCheckpoint,
            withdrawal.assetId,
            withdrawal.recipient,
            withdrawal.amount,
            availableAt
        );
    }

    function finaliseClaim(bytes32 claimId) external nonReentrant {
        Claim storage item = claim[claimId];
        if (item.status != ClaimStatus.Pending || block.timestamp < item.availableAt) revert ClaimNotReady();
        if (!challengeManager.claimable(item.checkpointHash)) {
            revert ChallengedCheckpoint();
        }
        item.status = ClaimStatus.Paid;
        pendingAmount[item.assetId] = Arithmetic.sub(pendingAmount[item.assetId], item.amount);
        nullifierRegistry.consume(item.nullifier, claimId);
        vault.release(claimId, item.assetId, item.recipient, item.amount);
        emit ClaimFinalised(claimId, item.nullifier);
    }

    function cancelChallengedClaim(bytes32 claimId) external nonReentrant {
        Claim storage item = claim[claimId];
        (,,,,, CheckpointChallengeManager.Status status) = challengeManager.challenge(item.checkpointHash);
        if (item.status != ClaimStatus.Pending || status != CheckpointChallengeManager.Status.Upheld) {
            revert ChallengedCheckpoint();
        }
        item.status = ClaimStatus.Cancelled;
        pendingAmount[item.assetId] = Arithmetic.sub(pendingAmount[item.assetId], item.amount);
        nullifierRegistry.cancel(item.nullifier, claimId);
        emit ClaimCancelled(claimId, item.nullifier);
    }

    function _verifyProof(bytes32 leaf, StateProof calldata proof, bytes32 expectedRoot) private pure returns (bool) {
        return PaxeerWithdrawalCodec.proofRoot(leaf, proof.leafIndex, proof.siblings) == expectedRoot;
    }
}
