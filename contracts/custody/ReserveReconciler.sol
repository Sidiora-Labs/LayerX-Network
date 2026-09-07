// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {CanonicalCheckpoint} from "../libraries/CanonicalCheckpoint.sol";
import {PaxeerWithdrawalCodec} from "../libraries/PaxeerWithdrawalCodec.sol";
import {SafeTransfer} from "../libraries/SafeTransfer.sol";
import {CheckpointRegistry} from "../CheckpointRegistry.sol";
import {LayerXVault} from "./LayerXVault.sol";
import {WithdrawalClaims} from "../WithdrawalClaims.sol";
import {Arithmetic} from "../libraries/Arithmetic.sol";
import {LayerXComponent} from "../security/LayerXComponent.sol";
import {Predeploys} from "../deployment/Predeploys.sol";

contract ReserveReconciler is LayerXComponent {
    struct LiabilityReport {
        bytes32 assetId;
        uint128 agentMain;
        uint128 escrow;
        uint128 budget;
        uint128 stream;
        uint128 margin;
        uint128 liquidity;
        uint128 insurance;
        uint128 fees;
        uint128 withdrawals;
        uint128 otherSystem;
        uint128 reserveMirror;
    }

    struct StateProof {
        uint256 leafIndex;
        bytes32[] siblings;
    }

    struct Reconciliation {
        bytes32 checkpointHash;
        uint256 custody;
        uint256 circulating;
        uint256 outstandingClaims;
        uint256 reserveMirror;
        uint64 reconciledAt;
    }

    error InvalidReserveProof();
    error ReserveDeficit();

    CheckpointRegistry public immutable registry;
    LayerXVault public immutable vault;
    WithdrawalClaims public immutable withdrawalClaims;
    mapping(bytes32 => Reconciliation) public latest;

    event ReserveReconciled(
        bytes32 indexed assetId,
        bytes32 indexed checkpointHash,
        uint256 custody,
        uint256 circulating,
        uint256 outstandingClaims,
        uint256 reserveMirror
    );

    constructor(
        CheckpointRegistry checkpointRegistry,
        LayerXVault custodyVault,
        WithdrawalClaims claims,
        bytes32 componentConfigHash,
        uint192 componentRelease
    ) LayerXComponent(Predeploys.RESERVE_RECONCILER, componentConfigHash, componentRelease) {
        if (
            address(checkpointRegistry) == address(0) || address(custodyVault) == address(0)
                || address(claims) == address(0)
        ) revert InvalidReserveProof();
        registry = checkpointRegistry;
        vault = custodyVault;
        withdrawalClaims = claims;
    }

    function liabilityLeaf(LiabilityReport calldata report) public pure returns (bytes32) {
        return sha256(
            abi.encodePacked(
                "LXP/v1/merkle-leaf\x00",
                report.assetId,
                report.agentMain,
                report.escrow,
                report.budget,
                report.stream,
                report.margin,
                report.liquidity,
                report.insurance,
                report.fees,
                report.withdrawals,
                report.otherSystem,
                report.reserveMirror
            )
        );
    }

    function reconcile(
        bytes32 checkpointHash,
        bytes32 stateRoot,
        LiabilityReport calldata report,
        StateProof calldata proof,
        CanonicalCheckpoint.GuarantorAttestation[] calldata recordedAttestations
    ) external returns (Reconciliation memory result) {
        if (
            checkpointHash != registry.checkpointAtBatch(registry.finalisedBatchNumber())
                || !registry.isFinalised(checkpointHash, stateRoot)
                || !registry.isRecordedCertificate(checkpointHash, recordedAttestations)
                || PaxeerWithdrawalCodec.proofRoot(liabilityLeaf(report), proof.leafIndex, proof.siblings) != stateRoot
        ) revert InvalidReserveProof();
        uint256 circulating = Arithmetic.add(
            Arithmetic.add(
                Arithmetic.add(
                    Arithmetic.add(Arithmetic.add(uint256(report.agentMain), report.escrow), report.budget),
                    report.stream
                ),
                report.margin
            ),
            report.liquidity
        );
        circulating = Arithmetic.add(circulating, report.insurance);
        circulating = Arithmetic.add(circulating, report.fees);
        circulating = Arithmetic.add(circulating, report.withdrawals);
        circulating = Arithmetic.add(circulating, report.otherSystem);
        uint256 pending = withdrawalClaims.pendingAmount(report.assetId);
        if (pending != report.withdrawals) revert InvalidReserveProof();
        address token = vault.assetRegistry().asset(report.assetId).token;
        uint256 custody = SafeTransfer.balanceOf(token, address(vault));
        if (custody < circulating || custody != Arithmetic.add(circulating, report.reserveMirror)) {
            revert ReserveDeficit();
        }
        result = Reconciliation({
            checkpointHash: checkpointHash,
            custody: custody,
            circulating: circulating,
            outstandingClaims: pending,
            reserveMirror: report.reserveMirror,
            reconciledAt: Arithmetic.toUint64(block.timestamp)
        });
        latest[report.assetId] = result;
        emit ReserveReconciled(report.assetId, checkpointHash, custody, circulating, pending, report.reserveMirror);
    }
}
