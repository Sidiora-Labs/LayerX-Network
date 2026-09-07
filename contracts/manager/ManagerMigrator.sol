// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {ManagerContainer} from "./ManagerContainer.sol";
import {StandardValidatorUtils} from "./StandardValidatorUtils.sol";
import {SafeCall} from "../libraries/SafeCall.sol";
import {SemverComp} from "../libraries/SemverComp.sol";
import {Arithmetic} from "../libraries/Arithmetic.sol";
import {ReentrancyLock} from "../security/ReentrancyLock.sol";
import {LayerXComponent} from "../security/LayerXComponent.sol";
import {Predeploys} from "../deployment/Predeploys.sol";
import {
    ManagerUnauthorized,
    InvalidManagerConfiguration,
    InvalidMigrationState,
    InvalidMigrationWindow,
    InvalidMigrationVersion,
    MigrationCommitmentMismatch,
    InvalidMigrationOperation,
    MigrationCallFailed
} from "./BlockErrors.sol";

contract ManagerMigrator is ReentrancyLock, LayerXComponent {
    enum State {
        None,
        Staged,
        Executing,
        Completed,
        Cancelled
    }

    struct Plan {
        uint192 sourceRelease;
        uint192 targetRelease;
        bytes32 staticConfigHash;
        bytes32 operationsRoot;
        uint64 executeAfter;
        uint64 expiresAt;
        uint16 operationCount;
        uint256 totalValue;
        State state;
    }

    ManagerContainer public immutable container;
    address public immutable governanceTimelock;
    address public immutable executor;
    uint64 public immutable minimumDelay;
    uint64 public immutable maximumValidity;
    uint64 public immutable maximumOperationGas;
    uint256 public immutable maximumCallValue;

    uint256 public migrationNonce;
    bytes32 public activeMigration;
    mapping(bytes32 => Plan) public migrationPlan;

    event MigrationStaged(
        bytes32 indexed migrationId,
        uint192 sourceRelease,
        uint192 targetRelease,
        bytes32 indexed operationsRoot,
        bytes32 indexed staticConfigHash,
        uint64 executeAfter,
        uint64 expiresAt,
        uint16 operationCount,
        uint256 totalValue
    );
    event MigrationOperationExecuted(
        bytes32 indexed migrationId,
        uint256 indexed operationIndex,
        bytes32 indexed role,
        address target,
        bytes4 selector,
        bytes32 codeHashAfter,
        uint16 storageLayoutAfter
    );
    event MigrationCompleted(bytes32 indexed migrationId, uint192 targetRelease);
    event MigrationCancelled(bytes32 indexed migrationId);

    constructor(
        ManagerContainer managerContainer,
        address governance,
        address migrationExecutor,
        uint64 delay,
        uint64 validity,
        uint64 operationGasLimit,
        uint256 callValueLimit
    )
        LayerXComponent(
            Predeploys.MANAGER_MIGRATOR, managerContainer.staticConfigHash(), managerContainer.currentRelease()
        )
    {
        if (
            address(managerContainer) == address(0) || governance == address(0) || migrationExecutor == address(0)
                || delay < 1 days || validity < 1 days || validity > 30 days || operationGasLimit < 100_000
                || operationGasLimit > 10_000_000 || callValueLimit > 100 ether
        ) {
            revert InvalidManagerConfiguration();
        }
        container = managerContainer;
        governanceTimelock = governance;
        executor = migrationExecutor;
        minimumDelay = delay;
        maximumValidity = validity;
        maximumOperationGas = operationGasLimit;
        maximumCallValue = callValueLimit;
    }

    receive() external payable {}

    modifier onlyGovernance() {
        if (msg.sender != governanceTimelock) {
            revert ManagerUnauthorized(msg.sender);
        }
        _;
    }

    function stageMigration(
        uint192 targetRelease,
        bytes32 configHash,
        StandardValidatorUtils.Operation[] calldata operations,
        uint64 delay,
        uint64 validity
    ) external onlyGovernance returns (bytes32 migrationId) {
        if (activeMigration != bytes32(0)) {
            revert InvalidMigrationState(activeMigration, uint8(migrationPlan[activeMigration].state));
        }
        uint192 sourceRelease = container.currentRelease();
        if (!SemverComp.isStrictUpgrade(sourceRelease, targetRelease)) {
            revert InvalidMigrationVersion(sourceRelease, targetRelease);
        }
        if (
            configHash != container.staticConfigHash() || container.migrator() != address(this)
                || operations.length == 0 || operations.length > container.roleCount()
        ) {
            revert InvalidManagerConfiguration();
        }
        if (delay < minimumDelay || validity < 1 days || validity > maximumValidity) {
            revert InvalidMigrationWindow(delay, validity);
        }

        uint256 totalValue;
        for (uint256 i = 0; i < operations.length; ++i) {
            _validateBefore(operations[i], i, configHash);
            totalValue = Arithmetic.add(totalValue, operations[i].value);
            for (uint256 j = 0; j < i; ++j) {
                if (operations[j].target == operations[i].target) {
                    revert InvalidMigrationOperation(i);
                }
            }
        }
        bytes32 root = StandardValidatorUtils.operationsRoot(operations);
        uint64 stagedAt = Arithmetic.toUint64(block.timestamp);
        uint64 executeAfter = Arithmetic.toUint64(uint256(stagedAt) + delay);
        uint64 expiresAt = Arithmetic.toUint64(uint256(executeAfter) + validity);
        uint256 nonce = migrationNonce++;
        migrationId = keccak256(
            abi.encode(
                "LXP/Paxeer/migration-plan/v1",
                block.chainid,
                address(this),
                address(container),
                nonce,
                sourceRelease,
                targetRelease,
                configHash,
                root,
                executeAfter,
                expiresAt,
                operations.length,
                totalValue
            )
        );
        migrationPlan[migrationId] = Plan({
            sourceRelease: sourceRelease,
            targetRelease: targetRelease,
            staticConfigHash: configHash,
            operationsRoot: root,
            executeAfter: executeAfter,
            expiresAt: expiresAt,
            operationCount: Arithmetic.toUint16(operations.length),
            totalValue: totalValue,
            state: State.Staged
        });
        activeMigration = migrationId;
        emit MigrationStaged(
            migrationId,
            sourceRelease,
            targetRelease,
            root,
            configHash,
            executeAfter,
            expiresAt,
            Arithmetic.toUint16(operations.length),
            totalValue
        );
    }

    function executeMigration(bytes32 migrationId, StandardValidatorUtils.Operation[] calldata operations)
        external
        nonReentrant
    {
        if (msg.sender != executor) revert ManagerUnauthorized(msg.sender);
        Plan storage plan = migrationPlan[migrationId];
        if (migrationId == bytes32(0) || activeMigration != migrationId || plan.state != State.Staged) {
            revert InvalidMigrationState(migrationId, uint8(plan.state));
        }
        if (block.timestamp < plan.executeAfter || block.timestamp > plan.expiresAt) {
            revert InvalidMigrationWindow(plan.executeAfter, plan.expiresAt);
        }
        bytes32 actualRoot = StandardValidatorUtils.operationsRoot(operations);
        if (operations.length != plan.operationCount || actualRoot != plan.operationsRoot) {
            revert MigrationCommitmentMismatch(plan.operationsRoot, actualRoot);
        }
        if (
            container.currentRelease() != plan.sourceRelease || container.staticConfigHash() != plan.staticConfigHash
                || address(this).balance < plan.totalValue
        ) {
            revert InvalidManagerConfiguration();
        }

        for (uint256 i = 0; i < operations.length; ++i) {
            _validateBefore(operations[i], i, plan.staticConfigHash);
        }

        plan.state = State.Executing;
        for (uint256 i = 0; i < operations.length; ++i) {
            StandardValidatorUtils.Operation calldata operation = operations[i];
            SafeCall.CallResult memory result =
                SafeCall.call(operation.target, operation.value, operation.data, operation.gasLimit, 512, true);
            if (!result.success) {
                revert MigrationCallFailed(i, operation.target, result.returnDataSize, result.returnData);
            }
            StandardValidatorUtils.validateCodeHash(operation, i, true);
            StandardValidatorUtils.validateStorageLayout(operation, i, true);
            StandardValidatorUtils.validateIdentity(operation, i, plan.staticConfigHash);
            emit MigrationOperationExecuted(
                migrationId,
                i,
                operation.role,
                operation.target,
                StandardValidatorUtils.selectorOf(operation.data),
                operation.target.codehash,
                operation.expectedStorageLayoutAfter
            );
        }
        container.completeMigration(migrationId, plan.targetRelease, plan.staticConfigHash);
        plan.state = State.Completed;
        activeMigration = bytes32(0);
        emit MigrationCompleted(migrationId, plan.targetRelease);
    }

    function cancelMigration(bytes32 migrationId) external onlyGovernance {
        Plan storage plan = migrationPlan[migrationId];
        if (activeMigration != migrationId || plan.state != State.Staged) {
            revert InvalidMigrationState(migrationId, uint8(plan.state));
        }
        plan.state = State.Cancelled;
        activeMigration = bytes32(0);
        emit MigrationCancelled(migrationId);
    }

    function operationRoot(StandardValidatorUtils.Operation[] calldata operations) external pure returns (bytes32) {
        return StandardValidatorUtils.operationsRoot(operations);
    }

    function _validateBefore(StandardValidatorUtils.Operation calldata operation, uint256 index, bytes32 configHash)
        private
        view
    {
        StandardValidatorUtils.validateStructure(operation, index, maximumCallValue, maximumOperationGas);
        container.requireAllowed(operation.role, operation.target, StandardValidatorUtils.selectorOf(operation.data));
        StandardValidatorUtils.validateCodeHash(operation, index, false);
        StandardValidatorUtils.validateStorageLayout(operation, index, false);
        StandardValidatorUtils.validateIdentity(operation, index, configHash);
    }
}
