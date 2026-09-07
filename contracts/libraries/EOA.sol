// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

library EOA {
    bytes32 internal constant EIP7702_DESIGNATOR_WORD =
        0xef01000000000000000000000000000000000000000000000000000000000000;
    bytes32 internal constant EIP7702_DESIGNATOR_MASK =
        0xffffff0000000000000000000000000000000000000000000000000000000000;
    uint256 internal constant EIP7702_CODE_LENGTH = 23;

    function isDirectCodelessOrigin() internal view returns (bool) {
        return msg.sender == tx.origin && msg.sender.code.length == 0;
    }

    function isDirectCodelessOrigin(address sender, address origin, uint256 senderCodeLength)
        internal
        pure
        returns (bool)
    {
        return sender == origin && senderCodeLength == 0;
    }

    function isDelegatedAccount(address account) internal view returns (bool) {
        if (account.code.length != EIP7702_CODE_LENGTH) return false;
        bytes32 head;
        assembly ("memory-safe") {
            extcodecopy(account, 0, 0, 3)
            head := mload(0)
        }
        return head & EIP7702_DESIGNATOR_MASK == EIP7702_DESIGNATOR_WORD;
    }

    function delegationTarget(address account) internal view returns (address target) {
        if (!isDelegatedAccount(account)) return address(0);
        assembly ("memory-safe") {
            extcodecopy(account, 0, 3, 20)
            target := shr(96, mload(0))
        }
    }

    function isContract(address account) internal view returns (bool) {
        return account.code.length != 0 && !isDelegatedAccount(account);
    }
}
