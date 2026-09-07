// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {Types} from "./Types.sol";

library Arithmetic {
    error ArithmeticOverflow();
    error ArithmeticUnderflow();
    error DivisionByZero();
    error NarrowingConversion();

    function add(uint256 a, uint256 b) internal pure returns (uint256 result) {
        unchecked {
            result = a + b;
            if (result < a) revert ArithmeticOverflow();
        }
    }

    function sub(uint256 a, uint256 b) internal pure returns (uint256 result) {
        if (b > a) revert ArithmeticUnderflow();
        unchecked {
            result = a - b;
        }
    }

    function mul(uint256 a, uint256 b) internal pure returns (uint256 result) {
        unchecked {
            result = a * b;
            if (a != 0 && result / a != b) revert ArithmeticOverflow();
        }
    }

    function div(uint256 numerator, uint256 denominator) internal pure returns (uint256) {
        if (denominator == 0) revert DivisionByZero();
        return numerator / denominator;
    }

    function ceilDiv(uint256 numerator, uint256 denominator) internal pure returns (uint256) {
        if (denominator == 0) revert DivisionByZero();
        if (numerator == 0) return 0;
        return add((numerator - 1) / denominator, 1);
    }

    function toUint128(uint256 value) internal pure returns (uint128 result) {
        if (value > type(uint128).max) revert NarrowingConversion();
        assembly ("memory-safe") { result := value }
    }

    function toUint64(uint256 value) internal pure returns (uint64 result) {
        if (value > type(uint64).max) revert NarrowingConversion();
        assembly ("memory-safe") { result := value }
    }

    function toUint32(uint256 value) internal pure returns (uint32 result) {
        if (value > type(uint32).max) revert NarrowingConversion();
        assembly ("memory-safe") { result := value }
    }

    function toUint16(uint256 value) internal pure returns (uint16 result) {
        if (value > type(uint16).max) revert NarrowingConversion();
        assembly ("memory-safe") { result := value }
    }

    function toUint8(uint256 value) internal pure returns (uint8 result) {
        if (value > type(uint8).max) revert NarrowingConversion();
        assembly ("memory-safe") { result := value }
    }

    function mulDiv(uint256 x, uint256 y, uint256 denominator) internal pure returns (uint256 result) {
        unchecked {
            uint256 least;
            uint256 most;
            assembly ("memory-safe") {
                let modular := mulmod(x, y, not(0))
                least := mul(x, y)
                most := sub(sub(modular, least), lt(modular, least))
            }

            if (most == 0) {
                if (denominator == 0) revert DivisionByZero();
                return least / denominator;
            }
            if (denominator <= most) {
                if (denominator == 0) revert DivisionByZero();
                revert ArithmeticOverflow();
            }

            uint256 remainder;
            assembly ("memory-safe") {
                remainder := mulmod(x, y, denominator)
                most := sub(most, gt(remainder, least))
                least := sub(least, remainder)
            }

            uint256 factor = denominator & (~denominator + 1);
            assembly ("memory-safe") {
                denominator := div(denominator, factor)
                least := div(least, factor)
                factor := add(div(sub(0, factor), factor), 1)
            }
            least |= most * factor;

            uint256 inverse = (3 * denominator) ^ 2;
            inverse *= 2 - denominator * inverse;
            inverse *= 2 - denominator * inverse;
            inverse *= 2 - denominator * inverse;
            inverse *= 2 - denominator * inverse;
            inverse *= 2 - denominator * inverse;
            inverse *= 2 - denominator * inverse;
            result = least * inverse;
        }
    }

    function mulDiv(uint256 x, uint256 y, uint256 denominator, Types.Rounding rounding)
        internal
        pure
        returns (uint256 result)
    {
        result = mulDiv(x, y, denominator);
        if (rounding == Types.Rounding.Up && mulmod(x, y, denominator) != 0) {
            result = add(result, 1);
        }
    }
}
