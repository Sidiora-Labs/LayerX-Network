// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {Ed25519Verifier} from "../crypto/Ed25519.sol";

library NativeStateProof {
    error WrongVersion();
    error WrongModule();
    error InvalidEncoding();
    error InvalidPath();
    error RootMismatch();

    function verify(bytes calldata proof, uint16 expectedModule, bytes32 stateRoot) internal pure {
        if (root(proof, expectedModule) != stateRoot) revert RootMismatch();
    }

    function root(bytes calldata proof, uint16 expectedModule) internal pure returns (bytes32) {
        if (proof.length < 26 || proof.length > 1051812) revert InvalidEncoding();
        if (uint16(bytes2(proof[:2])) != 2) revert WrongVersion();
        uint16 moduleId = uint16(bytes2(proof[2:4]));
        if (moduleId > 9 || moduleId != expectedModule) revert WrongModule();
        uint32 keyLength = number(proof, 4);
        if (keyLength == 0 || keyLength > 129) revert InvalidEncoding();
        uint256 valueAt = 8 + uint256(keyLength);
        uint32 valueLength = number(proof, valueAt);
        if (valueLength > 1048576) revert InvalidEncoding();
        uint256 cursor = valueAt + 4 + uint256(valueLength);
        if (cursor > proof.length) revert InvalidEncoding();
        bytes32 node = sha256(
            abi.encodePacked(
                "LXP/v1/state-leaf\x00", keyLength, valueLength, proof[8:valueAt], proof[valueAt + 4:cursor]
            )
        );
        if (moduleId == 0 && keyLength == 33 && proof[8] == 0x04) {
            (node, cursor) = fold(proof, cursor + 8, node, number(proof, cursor), number(proof, cursor + 4));
            node = sha256(abi.encodePacked("LXP/v1/state-leaf\x00", uint32(12), uint32(32), "account-tree", node));
        }
        uint32 index = number(proof, cursor);
        uint32 count = number(proof, cursor + 4);
        (node, cursor) = fold(proof, cursor + 8, node, index, count);
        node = sha256(abi.encodePacked("LXP/v1/state-leaf\x00", uint32(2), uint32(32), moduleId, node));
        count = number(proof, cursor);
        if (count < 9 || count > 10) revert WrongModule();
        (node, cursor) = fold(proof, cursor + 4, node, moduleId, count);
        if (cursor != proof.length) revert InvalidEncoding();
        return node;
    }

    function verifyBalance(
        Ed25519Verifier verifier,
        bytes calldata proof,
        bytes32 stateRoot,
        bytes32 account,
        bytes32 asset,
        uint128 balance,
        uint32 network,
        address recipient,
        bytes32 requestAnchor,
        bytes calldata signature
    ) internal view {
        verify(proof, 0, stateRoot);
        if (uint32(bytes4(proof[4:8])) != 33 || proof[8] != 0x04 || bytes32(proof[9:41]) != account) {
            revert InvalidEncoding();
        }
        uint256 length = number(proof, 41);
        bytes calldata value = proof[45:45 + length];
        if (value.length < 2) revert InvalidEncoding();
        uint256 nameLength = uint16(bytes2(value[:2]));
        if (nameLength == 0 || nameLength > 512 || value.length != 103 + nameLength) revert InvalidEncoding();
        uint256 at = 2 + nameLength;
        if (
            value[at] != 0x01 || value[at + 49] != 0x01 || uint8(value[at + 66]) > 1 || uint8(value[at + 67]) > 1
                || value[at + 100] != 0x01 || uint128(bytes16(value[at + 1:at + 17])) != balance
                || bytes32(value[at + 17:at + 49]) != asset || network == 0 || recipient == address(0)
                || requestAnchor == bytes32(0)
        ) revert InvalidEncoding();
        bytes32 authority = bytes32(value[at + 68:at + 100]);
        if (!verifier.verify(
                authority,
                abi.encodePacked(
                    "LX:SETTLE:RECIPIENT:v1", bytes1(0), network, account, asset, recipient, requestAnchor
                ),
                signature
            )) revert InvalidEncoding();
    }

    function verifyWithdrawal(
        bytes calldata proof,
        bytes32 stateRoot,
        uint32 network,
        bytes32 withdrawalId,
        bytes32 account,
        bytes32 asset,
        uint128 amount,
        address recipient,
        bytes32 requestAnchor,
        bytes32 nullifier
    ) internal pure {
        verify(proof, 1, stateRoot);
        if (
            number(proof, 4) != 43 || keccak256(proof[8:19]) != keccak256("withdrawal:")
                || bytes32(proof[19:51]) != nullifier || number(proof, 51) != 182
        ) revert InvalidEncoding();
        bytes calldata value = proof[55:237];
        if (
            uint16(bytes2(value[:2])) != 2 || uint32(bytes4(value[2:6])) != network
                || bytes32(value[6:38]) != withdrawalId || bytes32(value[38:70]) != account
                || bytes32(value[70:102]) != asset || uint128(bytes16(value[102:118])) != amount
                || bytes32(value[118:150]) != bytes32(uint256(uint160(recipient)))
                || bytes32(value[150:182]) != requestAnchor
        ) revert InvalidEncoding();
    }

    function number(bytes calldata proof, uint256 cursor) private pure returns (uint32) {
        if (cursor + 4 > proof.length) revert InvalidEncoding();
        return uint32(bytes4(proof[cursor:cursor + 4]));
    }

    function fold(bytes calldata proof, uint256 cursor, bytes32 node, uint32 index, uint32 count)
        private
        pure
        returns (bytes32, uint256)
    {
        if (cursor >= proof.length || count == 0 || index >= count) revert InvalidPath();
        uint8 depth = uint8(proof[cursor++]);
        if (depth > 32 || cursor + uint256(depth) * 32 > proof.length) revert InvalidPath();
        for (uint256 level; level < depth; ++level) {
            bytes32 sibling = bytes32(proof[cursor:cursor + 32]);
            if (count <= 1 || ((index ^ 1) >= count && sibling != node)) revert InvalidPath();
            node = index & 1 == 0
                ? sha256(abi.encodePacked("LXP/v1/state-node\x00", node, sibling))
                : sha256(abi.encodePacked("LXP/v1/state-node\x00", sibling, node));
            index /= 2;
            count = count / 2 + count % 2;
            cursor += 32;
        }
        if (count != 1) revert InvalidPath();
        return (node, cursor);
    }
}
