// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {Constants} from "./Constants.sol";

library MerkleLib {
    error MerkleProofTooDeep(uint256 depth);
    error MerkleIndexOutOfRange(uint256 index, uint256 depth);

    function hashLeaf(bytes memory canonicalLeaf) internal pure returns (bytes32) {
        return sha256(abi.encodePacked("LXP/v1/merkle-leaf\x00", canonicalLeaf));
    }

    function hashNode(bytes32 left, bytes32 right) internal pure returns (bytes32) {
        return sha256(abi.encodePacked("LXP/v1/merkle-node\x00", left, right));
    }

    function root(bytes32 leaf, uint256 leafIndex, bytes32[] calldata siblings) internal pure returns (bytes32 result) {
        uint256 depth = siblings.length;
        if (depth > Constants.MAX_MERKLE_DEPTH) {
            revert MerkleProofTooDeep(depth);
        }
        if (depth < 256 && leafIndex >> depth != 0) {
            revert MerkleIndexOutOfRange(leafIndex, depth);
        }
        result = leaf;
        for (uint256 i = 0; i < depth; ++i) {
            if (((leafIndex >> i) & 1) == 0) {
                result = hashNode(result, siblings[i]);
            } else {
                result = hashNode(siblings[i], result);
            }
        }
    }

    function verify(bytes32 expectedRoot, bytes32 leaf, uint256 leafIndex, bytes32[] calldata siblings)
        internal
        pure
        returns (bool)
    {
        return root(leaf, leafIndex, siblings) == expectedRoot;
    }
}
