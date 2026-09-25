// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {ReentrancyLock} from "../security/ReentrancyLock.sol";
import {SafeCall} from "../libraries/SafeCall.sol";
import {Arithmetic} from "../libraries/Arithmetic.sol";
import {Constants} from "../libraries/Constants.sol";

interface ISidioraProxyUpgrade {
    function upgradeToAndCall(address implementation, bytes calldata data) external payable;
}

contract SidioraProxyTimelock is ReentrancyLock {
    error Unauthorized();
    error InvalidOperation();
    error OperationNotReady();
    error CallFailed(bytes returnData);

    address public immutable proxy;
    address public immutable governanceAuthority;
    address public immutable executor;
    address public immutable guardian;
    uint64 public immutable minDelay;
    uint64 public immutable delayFloor;
    uint64 public immutable gracePeriod;
    mapping(address => mapping(bytes4 => bool)) public callPermission;
    mapping(bytes32 => uint64) public readyAt;
    mapping(bytes32 => bool) public completed;
    mapping(bytes32 => bytes32) private dataHashes;
    uint256 public operationNonce;

    event OperationScheduled(
        bytes32 indexed operationId, address indexed target, uint256 value, bytes32 dataHash, uint64 readyAt
    );
    event OperationCancelled(bytes32 indexed operationId, address indexed target, uint256 value, bytes32 dataHash);
    event OperationExecuted(bytes32 indexed operationId, address indexed target, uint256 value, bytes32 dataHash);

    constructor(
        address proxyAddress,
        address proposerAddress,
        address executorAddress,
        address guardianAddress,
        uint64 minimumDelay,
        uint64 minimumDelayFloor,
        uint64 executionGracePeriod
    ) {
        if (
            proxyAddress == address(0) || proxyAddress.code.length == 0 || proposerAddress == address(0)
                || executorAddress == address(0) || guardianAddress == address(0) || minimumDelayFloor == 0
                || minimumDelay < minimumDelayFloor || executionGracePeriod == 0
        ) revert InvalidOperation();
        proxy = proxyAddress;
        governanceAuthority = proposerAddress;
        executor = executorAddress;
        guardian = guardianAddress;
        minDelay = minimumDelay;
        delayFloor = minimumDelayFloor;
        gracePeriod = executionGracePeriod;
        callPermission[proxyAddress][ISidioraProxyUpgrade.upgradeToAndCall.selector] = true;
    }

    function proposer(address account) external view returns (bool) {
        return account == governanceAuthority;
    }

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
        if (msg.sender != governanceAuthority) revert Unauthorized();
        _validateCall(target, value, data);
        if (delay < minDelay) revert InvalidOperation();
        id = operationId(target, value, data, salt, operationNonce++);
        uint64 timestamp = Arithmetic.toUint64(block.timestamp + delay);
        readyAt[id] = timestamp;
        dataHashes[id] = sha256(data);
        emit OperationScheduled(id, target, value, sha256(data), timestamp);
    }

    function cancel(bytes32 id) external {
        if (msg.sender != guardian) revert Unauthorized();
        uint64 timestamp = readyAt[id];
        if (timestamp == 0 || completed[id] || block.timestamp >= timestamp) revert OperationNotReady();
        bytes32 dataHash = dataHashes[id];
        delete readyAt[id];
        delete dataHashes[id];
        emit OperationCancelled(id, proxy, 0, dataHash);
    }

    function execute(address target, uint256 value, bytes calldata data, bytes32 salt, uint256 nonce)
        external
        nonReentrant
        returns (bytes memory)
    {
        if (msg.sender != executor) revert Unauthorized();
        bytes32 id = operationId(target, value, data, salt, nonce);
        uint64 timestamp = readyAt[id];
        if (
            timestamp == 0 || completed[id] || block.timestamp < timestamp
                || block.timestamp > uint256(timestamp) + gracePeriod
        ) revert OperationNotReady();
        _validateCall(target, value, data);
        completed[id] = true;
        SafeCall.CallResult memory result =
            SafeCall.call(target, value, data, gasleft(), Constants.MAX_RETURN_DATA, true);
        if (!result.success) revert CallFailed(result.returnData);
        delete dataHashes[id];
        emit OperationExecuted(id, target, value, sha256(data));
        return result.returnData;
    }

    function _validateCall(address target, uint256 value, bytes calldata data) private view {
        if (
            target != proxy || target.code.length == 0 || value != 0 || data.length < 4
                || data.length > Constants.MAX_MIGRATION_CALLDATA || !callPermission[target][bytes4(data[:4])]
        ) revert InvalidOperation();
        (address implementation, bytes memory initialization) = abi.decode(data[4:], (address, bytes));
        if (
            implementation.code.length == 0 || implementation == proxy || implementation == address(this)
                || keccak256(data)
                    != keccak256(
                        abi.encodeCall(ISidioraProxyUpgrade.upgradeToAndCall, (implementation, initialization))
                    )
        ) revert InvalidOperation();
    }
}
