// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

library Bytes {
    error BytesOutOfBounds(uint256 offset, uint256 length, uint256 available);
    error TrailingBytes(uint256 consumed, uint256 available);

    function checkedEnd(uint256 offset, uint256 length, uint256 available) internal pure returns (uint256 end) {
        unchecked {
            end = offset + length;
            if (end < offset || end > available) {
                revert BytesOutOfBounds(offset, length, available);
            }
        }
    }

    function requireConsumed(uint256 consumed, uint256 available) internal pure {
        if (consumed != available) revert TrailingBytes(consumed, available);
    }

    function slice(bytes calldata data, uint256 offset, uint256 length) internal pure returns (bytes calldata result) {
        uint256 end = checkedEnd(offset, length, data.length);
        result = data[offset:end];
    }

    function readUint8(bytes calldata data, uint256 offset) internal pure returns (uint8 value) {
        checkedEnd(offset, 1, data.length);
        value = uint8(data[offset]);
    }

    function readUint16BE(bytes calldata data, uint256 offset) internal pure returns (uint16 value) {
        checkedEnd(offset, 2, data.length);
        assembly ("memory-safe") {
            value := shr(240, calldataload(add(data.offset, offset)))
        }
    }

    function readUint32BE(bytes calldata data, uint256 offset) internal pure returns (uint32 value) {
        checkedEnd(offset, 4, data.length);
        assembly ("memory-safe") {
            value := shr(224, calldataload(add(data.offset, offset)))
        }
    }

    function readUint64BE(bytes calldata data, uint256 offset) internal pure returns (uint64 value) {
        checkedEnd(offset, 8, data.length);
        assembly ("memory-safe") {
            value := shr(192, calldataload(add(data.offset, offset)))
        }
    }

    function readBytes32(bytes calldata data, uint256 offset) internal pure returns (bytes32 value) {
        checkedEnd(offset, 32, data.length);
        assembly ("memory-safe") {
            value := calldataload(add(data.offset, offset))
        }
    }

    function readAddress(bytes calldata data, uint256 offset) internal pure returns (address value) {
        checkedEnd(offset, 20, data.length);
        assembly ("memory-safe") {
            value := shr(96, calldataload(add(data.offset, offset)))
        }
    }
}
