// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {SafeCall} from "./SafeCall.sol";

library Burn {
    address internal constant NATIVE_BURN_SINK = 0x000000000000000000000000000000000000dEaD;

    error InvalidBurnAmount();
    event NativeBurned(address indexed source, uint256 amount);

    function native(uint256 amount) internal {
        if (amount == 0) revert InvalidBurnAmount();
        SafeCall.CallResult memory result = SafeCall.sendValue(NATIVE_BURN_SINK, amount, 30_000);
        SafeCall.requireSuccess(NATIVE_BURN_SINK, result);
        emit NativeBurned(address(this), amount);
    }
}
