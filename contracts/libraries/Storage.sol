// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

library Storage {
    bytes32 internal constant EIP1967_IMPLEMENTATION_SLOT =
        0x360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbc;
    bytes32 internal constant EIP1967_ADMIN_SLOT = 0xb53127684a568b3173ae13b9f8a6016e243e63b6e8ee1178d6a717850b5d6103;
    bytes32 internal constant EIP1967_BEACON_SLOT = 0xa3f0ad74e5423aebfd80d3ef4346578335a9a72aeaee59ff6cb3582b35133d50;

    error InvalidStorageNamespace();
    error ReservedStorageSlot(bytes32 slot);

    function derive(bytes32 component, bytes32 field) internal pure returns (bytes32 slot) {
        if (component == bytes32(0) || field == bytes32(0)) {
            revert InvalidStorageNamespace();
        }
        slot = keccak256(abi.encode("LXP/Paxeer/storage/v1", component, field));
        validate(slot);
    }

    function validate(bytes32 slot) internal pure {
        if (slot == bytes32(0)) revert InvalidStorageNamespace();
        if (slot == EIP1967_IMPLEMENTATION_SLOT || slot == EIP1967_ADMIN_SLOT || slot == EIP1967_BEACON_SLOT) {
            revert ReservedStorageSlot(slot);
        }
    }

    function loadBytes32(bytes32 slot) internal view returns (bytes32 value) {
        validate(slot);
        assembly ("memory-safe") { value := sload(slot) }
    }

    function storeBytes32(bytes32 slot, bytes32 value) internal {
        validate(slot);
        assembly ("memory-safe") { sstore(slot, value) }
    }

    function loadUint256(bytes32 slot) internal view returns (uint256 value) {
        validate(slot);
        assembly ("memory-safe") { value := sload(slot) }
    }

    function storeUint256(bytes32 slot, uint256 value) internal {
        validate(slot);
        assembly ("memory-safe") { sstore(slot, value) }
    }

    function loadAddress(bytes32 slot) internal view returns (address value) {
        validate(slot);
        assembly ("memory-safe") { value := sload(slot) }
    }

    function storeAddress(bytes32 slot, address value) internal {
        validate(slot);
        assembly ("memory-safe") { sstore(slot, value) }
    }
}
