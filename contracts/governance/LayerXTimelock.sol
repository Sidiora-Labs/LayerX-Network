// SPDX-License-Identifier: LicenseRef-Centra-ai-Protocol
pragma solidity ^0.8.24;

import {ReentrancyLock} from "../security/ReentrancyLock.sol";
import {LayerXComponent} from "../security/LayerXComponent.sol";
import {Predeploys} from "../deployment/Predeploys.sol";
import {SafeCall} from "../libraries/SafeCall.sol";
import {Arithmetic} from "../libraries/Arithmetic.sol";
import {Constants} from "../libraries/Constants.sol";
import {Error} from "../libraries/Error.sol";

abstract contract LayerXTimelockCore is ReentrancyLock, LayerXComponent {
    error Unauthorized();
    error InvalidOperation();
    error OperationNotReady();
    error CallFailed(bytes4 selector, bytes32 commitment, bytes returnData);

    uint64 public minDelay;
    uint64 public immutable delayFloor;
    uint64 public immutable gracePeriod;
    uint256 public immutable maximumCallValue;
    mapping(address => bool) public proposer;
    mapping(address => bool) public executor;
    mapping(address => bool) public guardian;
    mapping(bytes32 => uint64) public readyAt;
    mapping(bytes32 => bool) public completed;
    mapping(address => mapping(bytes4 => bool)) public callPermission;
    uint256 public operationNonce;

    event OperationScheduled(
        bytes32 indexed operationId, address indexed target, uint256 value, bytes32 dataHash, uint64 readyAt
    );
    event OperationCancelled(bytes32 indexed operationId);
    event OperationExecuted(bytes32 indexed operationId);
    event CallPermissionSet(address indexed target, bytes4 indexed selector, bool allowed);

    constructor(
        uint64 minimumDelay,
        uint64 executionGracePeriod,
        address initialProposer,
        address initialExecutor,
        address initialGuardian,
        uint256 callValueLimit,
        bytes32 componentConfigHash,
        uint192 componentRelease,
        uint64 minimumDelayFloor
    ) LayerXComponent(Predeploys.TIMELOCK, componentConfigHash, componentRelease) {
        if (
            minimumDelay < minimumDelayFloor || executionGracePeriod < 1 days || initialProposer == address(0)
                || initialExecutor == address(0) || initialGuardian == address(0) || callValueLimit > 100 ether
        ) {
            revert InvalidOperation();
        }
        delayFloor = minimumDelayFloor;
        minDelay = minimumDelay;
        gracePeriod = executionGracePeriod;
        maximumCallValue = callValueLimit;
        proposer[initialProposer] = true;
        executor[initialExecutor] = true;
        guardian[initialGuardian] = true;
    }

    receive() external payable {}

    function operationId(address target, uint256 value, bytes calldata data, bytes32 salt, uint256 nonce)
        public
        view
        returns (bytes32)
    {
        return sha256(abi.encode(block.chainid, address(this), target, value, sha256(data), salt, nonce));
    }

    function schedule(address target, uint256 value, bytes calldata data, bytes32 salt, uint64 delay)
        external
        returns (bytes32 id)
    {
        if (!proposer[msg.sender]) revert Unauthorized();
        if (
            target.code.length == 0 || data.length < 4 || data.length > Constants.MAX_MIGRATION_CALLDATA
                || value > maximumCallValue || delay < minDelay || !_isAllowed(target, _selector(data))
        ) {
            revert InvalidOperation();
        }
        uint256 nonce = operationNonce++;
        id = operationId(target, value, data, salt, nonce);
        uint64 timestamp = Arithmetic.toUint64(block.timestamp + delay);
        readyAt[id] = timestamp;
        emit OperationScheduled(id, target, value, sha256(data), timestamp);
    }

    function cancel(bytes32 id) external {
        if (!guardian[msg.sender] || readyAt[id] == 0 || completed[id]) {
            revert Unauthorized();
        }
        delete readyAt[id];
        emit OperationCancelled(id);
    }

    function execute(address target, uint256 value, bytes calldata data, bytes32 salt, uint256 nonce)
        external
        nonReentrant
        returns (bytes memory result)
    {
        if (!executor[msg.sender]) revert Unauthorized();
        bytes32 id = operationId(target, value, data, salt, nonce);
        uint64 timestamp = readyAt[id];
        if (
            timestamp == 0 || completed[id] || block.timestamp < timestamp
                || block.timestamp > uint256(timestamp) + gracePeriod
        ) {
            revert OperationNotReady();
        }
        if (target.code.length == 0 || value > maximumCallValue || !_isAllowed(target, _selector(data))) {
            revert InvalidOperation();
        }
        completed[id] = true;
        SafeCall.CallResult memory callResult =
            SafeCall.call(target, value, data, gasleft(), Constants.MAX_RETURN_DATA, true);
        if (!callResult.success) {
            revert CallFailed(
                Error.selector(callResult.returnData),
                Error.commitment(target, callResult.returnData),
                callResult.returnData
            );
        }
        emit OperationExecuted(id);
        return callResult.returnData;
    }

    function setRole(uint8 role, address account, bool enabled) external {
        if (msg.sender != address(this) || account == address(0)) {
            revert Unauthorized();
        }
        if (role == 1) proposer[account] = enabled;
        else if (role == 2) executor[account] = enabled;
        else if (role == 3) guardian[account] = enabled;
        else revert InvalidOperation();
    }

    function updateMinDelay(uint64 newDelay) external {
        if (msg.sender != address(this) || newDelay < delayFloor) {
            revert Unauthorized();
        }
        minDelay = newDelay;
    }

    function setCallPermission(address target, bytes4 selector, bool allowed) external {
        if (msg.sender != address(this) || target.code.length == 0 || selector == bytes4(0)) revert Unauthorized();
        callPermission[target][selector] = allowed;
        emit CallPermissionSet(target, selector, allowed);
    }

    function _isAllowed(address target, bytes4 selector) private view returns (bool) {
        if (target == address(this)) {
            return selector == this.setRole.selector || selector == this.updateMinDelay.selector
                || selector == this.setCallPermission.selector;
        }
        return callPermission[target][selector];
    }

    function _selector(bytes calldata data) private pure returns (bytes4 selector) {
        if (data.length < 4) revert InvalidOperation();
        assembly ("memory-safe") { selector := calldataload(data.offset) }
    }
}

contract LayerXTimelock is LayerXTimelockCore {
    constructor(
        uint64 minimumDelay,
        uint64 executionGracePeriod,
        address initialProposer,
        address initialExecutor,
        address initialGuardian,
        uint256 callValueLimit,
        bytes32 componentConfigHash,
        uint192 componentRelease
    )
        LayerXTimelockCore(
            minimumDelay,
            executionGracePeriod,
            initialProposer,
            initialExecutor,
            initialGuardian,
            callValueLimit,
            componentConfigHash,
            componentRelease,
            1 days
        )
    {}
}
