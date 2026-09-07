// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

error ManagerUnauthorized(address caller);
error ManagerNotInitialized();
error ManagerAlreadyInitialized();
error InvalidManagerConfiguration();
error InvalidComponent(bytes32 role, address component);
error DuplicateComponent(bytes32 role, address component);
error SelectorNotAllowed(bytes32 role, bytes4 selector);
error InvalidMigrationState(bytes32 migrationId, uint8 state);
error InvalidMigrationWindow(uint64 executeAfter, uint64 expiresAt);
error InvalidMigrationVersion(uint192 source, uint192 target);
error MigrationCommitmentMismatch(bytes32 expected, bytes32 actual);
error InvalidMigrationOperation(uint256 index);
error MigrationCodeHashMismatch(uint256 index, address target, bytes32 expected, bytes32 actual);
error MigrationStorageLayoutMismatch(uint256 index, uint16 expected, uint16 actual);
error MigrationCallFailed(uint256 index, address target, uint256 returnDataSize, bytes returnData);

library BlockErrors {
    function unauthorized() internal pure returns (bytes4) {
        return ManagerUnauthorized.selector;
    }

    function invalidComponent() internal pure returns (bytes4) {
        return InvalidComponent.selector;
    }

    function invalidMigrationState() internal pure returns (bytes4) {
        return InvalidMigrationState.selector;
    }

    function invalidMigrationOperation() internal pure returns (bytes4) {
        return InvalidMigrationOperation.selector;
    }

    function migrationCallFailed() internal pure returns (bytes4) {
        return MigrationCallFailed.selector;
    }
}
