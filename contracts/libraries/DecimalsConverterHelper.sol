// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {Arithmetic} from "./Arithmetic.sol";
import {Constants} from "./Constants.sol";
import {Types} from "./Types.sol";

library DecimalsConverterHelper {
    error UnsupportedDecimals(uint8 decimals);
    error InexactConversion(uint256 remainder);

    function scaleFactor(uint8 difference) internal pure returns (uint256) {
        if (difference > Constants.MAX_TOKEN_DECIMALS) {
            revert UnsupportedDecimals(difference);
        }
        uint256 factor = 1;
        for (uint256 i = 0; i < difference; ++i) {
            factor = Arithmetic.mul(factor, 10);
        }
        return factor;
    }

    function convert(uint256 amount, uint8 sourceDecimals, uint8 targetDecimals)
        internal
        pure
        returns (Types.DecimalConversion memory result)
    {
        if (sourceDecimals > Constants.MAX_TOKEN_DECIMALS) {
            revert UnsupportedDecimals(sourceDecimals);
        }
        if (targetDecimals > Constants.MAX_TOKEN_DECIMALS) {
            revert UnsupportedDecimals(targetDecimals);
        }
        if (sourceDecimals == targetDecimals) {
            return Types.DecimalConversion(amount, 0);
        }
        if (sourceDecimals < targetDecimals) {
            uint256 factor = scaleFactor(targetDecimals - sourceDecimals);
            return Types.DecimalConversion(Arithmetic.mul(amount, factor), 0);
        }
        uint256 divisor = scaleFactor(sourceDecimals - targetDecimals);
        result.converted = amount / divisor;
        result.remainder = amount % divisor;
    }

    function convertExact(uint256 amount, uint8 sourceDecimals, uint8 targetDecimals)
        internal
        pure
        returns (uint256 converted)
    {
        Types.DecimalConversion memory result = convert(amount, sourceDecimals, targetDecimals);
        if (result.remainder != 0) {
            revert InexactConversion(result.remainder);
        }
        return result.converted;
    }
}
