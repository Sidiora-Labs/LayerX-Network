// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {Constants} from "./Constants.sol";

library SafeCall {
    struct CallResult {
        bool success;
        uint256 returnDataSize;
        bytes returnData;
    }

    error TargetHasNoCode(address target);
    error InvalidCopyLimit(uint256 limit);
    error InvalidGasLimit(uint256 limit);
    error InsufficientNativeBalance(uint256 available, uint256 required);
    error CallFailed(address target, uint256 returnDataSize, bytes returnData);

    function call(
        address target,
        uint256 value,
        bytes memory input,
        uint256 gasLimit,
        uint32 maximumCopy,
        bool requireCode
    ) internal returns (CallResult memory result) {
        _validate(target, value, gasLimit, maximumCopy, requireCode);
        bool success;
        uint256 size;
        assembly ("memory-safe") {
            success := call(gasLimit, target, value, add(input, 32), mload(input), 0, 0)
            size := returndatasize()
        }
        result = _copy(success, size, maximumCopy);
    }

    function staticCall(address target, bytes memory input, uint256 gasLimit, uint32 maximumCopy)
        internal
        view
        returns (CallResult memory result)
    {
        _validate(target, 0, gasLimit, maximumCopy, true);
        bool success;
        uint256 size;
        assembly ("memory-safe") {
            success := staticcall(gasLimit, target, add(input, 32), mload(input), 0, 0)
            size := returndatasize()
        }
        result = _copy(success, size, maximumCopy);
    }

    function sendValue(address recipient, uint256 value, uint256 gasLimit) internal returns (CallResult memory) {
        return call(recipient, value, "", gasLimit, 32, false);
    }

    function requireSuccess(address target, CallResult memory result) internal pure returns (bytes memory) {
        if (!result.success) {
            revert CallFailed(target, result.returnDataSize, result.returnData);
        }
        return result.returnData;
    }

    function _validate(address target, uint256 value, uint256 gasLimit, uint32 maximumCopy, bool requireCode)
        private
        view
    {
        if (target == address(0) || (requireCode && target.code.length == 0)) {
            revert TargetHasNoCode(target);
        }
        if (maximumCopy == 0 || maximumCopy > Constants.MAX_RETURN_DATA) {
            revert InvalidCopyLimit(maximumCopy);
        }
        if (gasLimit < 5_000) revert InvalidGasLimit(gasLimit);
        if (value > address(this).balance) {
            revert InsufficientNativeBalance(address(this).balance, value);
        }
    }

    function _copy(bool success, uint256 size, uint32 maximumCopy) private pure returns (CallResult memory result) {
        uint256 copiedLength = size;
        if (copiedLength > maximumCopy) copiedLength = maximumCopy;
        bytes memory copied = new bytes(copiedLength);
        assembly ("memory-safe") {
            returndatacopy(add(copied, 32), 0, copiedLength)
        }
        result = CallResult(success, size, copied);
    }
}
