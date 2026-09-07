// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {EOA} from "./EOA.sol";

library CallerChecker {
    error DirectCodelessOriginOnly(address sender, address origin);
    error ContractCallerOnly(address sender);
    error DelegatedAccountNotAllowed(address sender, address delegate);

    function requireDirectCodelessOrigin() internal view {
        if (!EOA.isDirectCodelessOrigin()) {
            revert DirectCodelessOriginOnly(msg.sender, tx.origin);
        }
    }

    function requireContractCaller() internal view {
        if (!EOA.isContract(msg.sender)) {
            revert ContractCallerOnly(msg.sender);
        }
    }

    function requireNotDelegatedAccount() internal view {
        address delegate = EOA.delegationTarget(msg.sender);
        if (delegate != address(0)) {
            revert DelegatedAccountNotAllowed(msg.sender, delegate);
        }
    }
}
