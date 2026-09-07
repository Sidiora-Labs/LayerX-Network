// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {IERC20Minimal} from "../interfaces/IERC20Minimal.sol";
import {SafeCall} from "./SafeCall.sol";

library SafeTransfer {
    error TokenCallFailed();
    error TokenBalanceReadFailed();

    function safeTransfer(address token, address recipient, uint256 amount) internal {
        SafeCall.CallResult memory result =
            SafeCall.call(token, 0, abi.encodeCall(IERC20Minimal.transfer, (recipient, amount)), gasleft(), 32, true);
        if (
            !result.success
                || (result.returnDataSize != 0
                    && (result.returnDataSize != 32 || !abi.decode(result.returnData, (bool))))
        ) {
            revert TokenCallFailed();
        }
    }

    function safeTransferFrom(address token, address sender, address recipient, uint256 amount) internal {
        SafeCall.CallResult memory result = SafeCall.call(
            token, 0, abi.encodeCall(IERC20Minimal.transferFrom, (sender, recipient, amount)), gasleft(), 32, true
        );
        if (
            !result.success
                || (result.returnDataSize != 0
                    && (result.returnDataSize != 32 || !abi.decode(result.returnData, (bool))))
        ) {
            revert TokenCallFailed();
        }
    }

    function balanceOf(address token, address account) internal view returns (uint256 balance) {
        SafeCall.CallResult memory result =
            SafeCall.staticCall(token, abi.encodeCall(IERC20Minimal.balanceOf, (account)), gasleft(), 32);
        if (!result.success || result.returnDataSize != 32) {
            revert TokenBalanceReadFailed();
        }
        balance = abi.decode(result.returnData, (uint256));
    }
}
