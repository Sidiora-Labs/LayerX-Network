// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {MerkleLib} from "./MerkleLib.sol";

library PaxeerWithdrawalCodec {
    function nullifier(
        uint32 networkId,
        bytes32 withdrawalId,
        bytes32 account,
        bytes32 assetId,
        uint128 amount,
        bytes32 checkpointHash
    ) internal pure returns (bytes32) {
        return sha256(
            abi.encodePacked("LX:WITHDRAWAL:v1", networkId, withdrawalId, account, assetId, amount, checkpointHash)
        );
    }

    function withdrawalLeaf(bytes32 withdrawalId, bytes32 account, bytes32 assetId, uint128 amount, address recipient)
        internal
        pure
        returns (bytes32)
    {
        return sha256(
            abi.encodePacked(
                "LXP/v1/merkle-leaf\x00", withdrawalId, account, assetId, amount, bytes32(uint256(uint160(recipient)))
            )
        );
    }

    function balanceLeaf(bytes32 account, bytes32 assetId, uint128 balance, address recipient)
        internal
        pure
        returns (bytes32)
    {
        return sha256(
            abi.encodePacked("LXP/v1/merkle-leaf\x00", account, assetId, balance, bytes32(uint256(uint160(recipient))))
        );
    }

    function proofRoot(bytes32 leaf, uint256 leafIndex, bytes32[] calldata siblings) internal pure returns (bytes32) {
        return MerkleLib.root(leaf, leafIndex, siblings);
    }
}
