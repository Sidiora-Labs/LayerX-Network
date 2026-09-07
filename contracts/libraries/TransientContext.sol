// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {Storage} from "./Storage.sol";

abstract contract TransientContext {
    error InvalidContext();
    error ContextAlreadyActive(bytes32 activeContext);

    bool public immutable transientStorageEnabled;
    bytes32 private immutable contextStorageSlot;

    constructor(bool transientEnabled, bytes32 componentNamespace) {
        transientStorageEnabled = transientEnabled;
        contextStorageSlot = Storage.derive(componentNamespace, keccak256("execution-context"));
    }

    modifier scopedContext(bytes32 value) {
        if (value == bytes32(0)) revert InvalidContext();
        bytes32 active = _context();
        if (active != bytes32(0)) revert ContextAlreadyActive(active);
        _storeContext(value);
        _;
        _storeContext(bytes32(0));
    }

    function executionContext() public view returns (bytes32) {
        return _context();
    }

    function _context() internal view returns (bytes32 value) {
        bytes32 slot = contextStorageSlot;
        if (transientStorageEnabled) {
            assembly ("memory-safe") { value := tload(slot) }
        } else {
            value = Storage.loadBytes32(slot);
        }
    }

    function _storeContext(bytes32 value) private {
        bytes32 slot = contextStorageSlot;
        if (transientStorageEnabled) {
            assembly ("memory-safe") { tstore(slot, value) }
        } else {
            Storage.storeBytes32(slot, value);
        }
    }
}
