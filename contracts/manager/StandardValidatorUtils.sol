// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {ILayerXComponent} from "../deployment/Preinstalls.sol";
import {Predeploys} from "../deployment/Predeploys.sol";
import {SafeCall} from "../libraries/SafeCall.sol";
import {Constants} from "../libraries/Constants.sol";
import {InvalidMigrationOperation, MigrationCodeHashMismatch, MigrationStorageLayoutMismatch} from "./BlockErrors.sol";

library StandardValidatorUtils {
    struct Operation {
        bytes32 role;
        address target;
        uint256 value;
        uint64 gasLimit;
        bytes data;
        bytes32 expectedCodeHashBefore;
        bytes32 expectedCodeHashAfter;
        uint16 expectedStorageLayoutBefore;
        uint16 expectedStorageLayoutAfter;
    }

    function selectorOf(bytes calldata data) internal pure returns (bytes4 selector) {
        if (data.length < 4) revert InvalidMigrationOperation(0);
        assembly ("memory-safe") { selector := calldataload(data.offset) }
    }

    function selectorOfMemory(bytes memory data) internal pure returns (bytes4 selector) {
        if (data.length < 4) revert InvalidMigrationOperation(0);
        assembly ("memory-safe") { selector := mload(add(data, 32)) }
    }

    function validateStructure(
        Operation calldata operation,
        uint256 index,
        uint256 maximumCallValue,
        uint64 maximumGasLimit
    ) internal pure {
        if (
            !Predeploys.isKnown(operation.role) || operation.target == address(0) || operation.value > maximumCallValue
                || operation.gasLimit < 25_000 || operation.gasLimit > maximumGasLimit || operation.data.length < 4
                || operation.data.length > Constants.MAX_MIGRATION_CALLDATA
                || operation.expectedCodeHashBefore == bytes32(0) || operation.expectedCodeHashAfter == bytes32(0)
                || operation.expectedStorageLayoutBefore == 0
                || operation.expectedStorageLayoutAfter < operation.expectedStorageLayoutBefore
                || operation.expectedStorageLayoutAfter > operation.expectedStorageLayoutBefore + 1
        ) {
            revert InvalidMigrationOperation(index);
        }
    }

    function operationHash(Operation calldata operation, uint256 index) internal pure returns (bytes32) {
        return keccak256(
            abi.encode(
                "LXP/Paxeer/migration-operation/v1",
                index,
                operation.role,
                operation.target,
                operation.value,
                operation.gasLimit,
                keccak256(operation.data),
                selectorOf(operation.data),
                operation.expectedCodeHashBefore,
                operation.expectedCodeHashAfter,
                operation.expectedStorageLayoutBefore,
                operation.expectedStorageLayoutAfter
            )
        );
    }

    function operationsRoot(Operation[] calldata operations) internal pure returns (bytes32 root) {
        root = keccak256("LXP/Paxeer/migration-operations/v1");
        for (uint256 i = 0; i < operations.length; ++i) {
            root = keccak256(abi.encode(root, operationHash(operations[i], i)));
        }
    }

    function validateCodeHash(Operation calldata operation, uint256 index, bool afterCall) internal view {
        bytes32 expected = afterCall ? operation.expectedCodeHashAfter : operation.expectedCodeHashBefore;
        bytes32 actual = operation.target.codehash;
        if (operation.target.code.length == 0 || actual != expected) {
            revert MigrationCodeHashMismatch(index, operation.target, expected, actual);
        }
    }

    function validateStorageLayout(Operation calldata operation, uint256 index, bool afterCall) internal view {
        uint16 expected = afterCall ? operation.expectedStorageLayoutAfter : operation.expectedStorageLayoutBefore;
        SafeCall.CallResult memory result = SafeCall.staticCall(
            operation.target, abi.encodeCall(ILayerXComponent.storageLayoutVersion, ()), 50_000, 32
        );
        if (!result.success || result.returnDataSize != 32) {
            revert MigrationStorageLayoutMismatch(index, expected, 0);
        }
        uint256 decoded;
        bytes memory data = result.returnData;
        assembly ("memory-safe") { decoded := mload(add(data, 32)) }
        uint16 actual;
        if (decoded <= type(uint16).max) {
            assembly ("memory-safe") { actual := decoded }
        }
        if (decoded > type(uint16).max || actual != expected) {
            revert MigrationStorageLayoutMismatch(index, expected, actual);
        }
    }

    function validateIdentity(Operation calldata operation, uint256 index, bytes32 expectedConfigHash) internal view {
        bytes32 actualRole = _readWord(operation.target, ILayerXComponent.componentRole.selector, index);
        bytes32 actualConfig = _readWord(operation.target, ILayerXComponent.staticConfigHash.selector, index);
        if (actualRole != operation.role || actualConfig != expectedConfigHash) {
            revert InvalidMigrationOperation(index);
        }
    }

    function _readWord(address target, bytes4 selector, uint256 index) private view returns (bytes32 value) {
        SafeCall.CallResult memory result = SafeCall.staticCall(target, abi.encodeWithSelector(selector), 50_000, 32);
        if (!result.success || result.returnDataSize != 32) {
            revert InvalidMigrationOperation(index);
        }
        bytes memory data = result.returnData;
        assembly ("memory-safe") { value := mload(add(data, 32)) }
    }
}
