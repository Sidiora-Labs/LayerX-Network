// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {Constants} from "./Constants.sol";

library Error {
    enum Kind {
        Empty,
        RevertString,
        Panic,
        Custom
    }

    bytes4 internal constant REVERT_STRING_SELECTOR = 0x08c379a0;
    bytes4 internal constant PANIC_SELECTOR = 0x4e487b71;

    function selector(bytes memory returnData) internal pure returns (bytes4 value) {
        if (returnData.length < 4) return bytes4(0);
        assembly ("memory-safe") {
            value := mload(add(returnData, 32))
        }
    }

    function classify(bytes memory returnData) internal pure returns (Kind) {
        bytes4 value = selector(returnData);
        if (value == bytes4(0)) return Kind.Empty;
        if (value == REVERT_STRING_SELECTOR) return Kind.RevertString;
        if (value == PANIC_SELECTOR) return Kind.Panic;
        return Kind.Custom;
    }

    function commitment(address target, bytes memory returnData) internal pure returns (bytes32) {
        return sha256(
            abi.encode(
                Constants.DOMAIN_ERROR, Constants.PROTOCOL_VERSION, target, returnData.length, sha256(returnData)
            )
        );
    }
}
