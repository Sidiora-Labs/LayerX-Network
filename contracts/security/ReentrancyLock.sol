// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

abstract contract ReentrancyLock {
    error Reentrancy();
    uint256 private reentrancyState = 1;

    modifier nonReentrant() {
        if (reentrancyState != 1) revert Reentrancy();
        reentrancyState = 2;
        _;
        reentrancyState = 1;
    }
}
