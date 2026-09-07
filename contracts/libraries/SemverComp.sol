// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {Arithmetic} from "./Arithmetic.sol";

library SemverComp {
    error InvalidSemanticVersion();
    error SemanticVersionOverflow();

    function parseRelease(string memory text) internal pure returns (uint192 packed) {
        bytes memory raw = bytes(text);
        if (raw.length < 5 || raw.length > 62) {
            revert InvalidSemanticVersion();
        }
        uint64[3] memory parts;
        uint256 cursor;
        for (uint256 field = 0; field < 3; ++field) {
            uint256 start = cursor;
            uint256 value;
            while (cursor < raw.length && raw[cursor] >= 0x30 && raw[cursor] <= 0x39) {
                value = value * 10 + uint8(raw[cursor]) - 48;
                if (value > type(uint64).max) {
                    revert SemanticVersionOverflow();
                }
                ++cursor;
            }
            if (cursor == start || (cursor - start > 1 && raw[start] == 0x30)) {
                revert InvalidSemanticVersion();
            }
            parts[field] = Arithmetic.toUint64(value);
            if (field < 2) {
                if (cursor >= raw.length || raw[cursor] != 0x2e) {
                    revert InvalidSemanticVersion();
                }
                ++cursor;
            } else if (cursor != raw.length) {
                revert InvalidSemanticVersion();
            }
        }
        packed = (uint192(parts[0]) << 128) | (uint192(parts[1]) << 64) | uint192(parts[2]);
    }

    function major(uint192 version) internal pure returns (uint64 result) {
        assembly ("memory-safe") { result := shr(128, version) }
    }

    function minor(uint192 version) internal pure returns (uint64 result) {
        assembly ("memory-safe") {
            result := and(shr(64, version), 0xffffffffffffffff)
        }
    }

    function patch(uint192 version) internal pure returns (uint64 result) {
        assembly ("memory-safe") {
            result := and(version, 0xffffffffffffffff)
        }
    }

    function compare(uint192 first, uint192 second) internal pure returns (int8) {
        if (first < second) return -1;
        if (first > second) return 1;
        return 0;
    }

    function isStrictUpgrade(uint192 current, uint192 proposed) internal pure returns (bool) {
        return proposed > current;
    }
}
