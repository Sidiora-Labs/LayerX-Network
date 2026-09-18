// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {MerkleLib} from "../contracts/libraries/MerkleLib.sol";

contract MerkleVectorHarness {
    function proofRoot(bytes32 leaf, uint256 leafIndex, bytes32[] calldata siblings) external pure returns (bytes32) {
        return MerkleLib.root(leaf, leafIndex, siblings);
    }
}

/// Cross-language Merkle vector.
///
/// Every constant below is produced by the native C implementation in
/// src/crypto/lxp_merkle.c and printed by tests/crypto/lxp_test_merkle_vector.c
/// (`make test-merkle-vector`), which asserts the same bytes before printing
/// them. The daemon commits to those digests, so MerkleLib.sol must reproduce
/// them exactly or on-chain verification of a native proof cannot succeed.
contract MerkleDomainVectorTest {
    MerkleVectorHarness private harness;

    bytes private constant LEAF_VALUE_0 = "layerx/merkle/vector/0";
    bytes private constant LEAF_VALUE_1 = "layerx/merkle/vector/1";
    bytes private constant LEAF_VALUE_2 = "layerx/merkle/vector/2";
    bytes private constant LEAF_VALUE_3 = "layerx/merkle/vector/3";

    bytes32 private constant LEAF_HASH_0 = 0x586342bf39f9dda925f420ff23446f027e3e20565362a151bd758196e6f696b9;
    bytes32 private constant LEAF_HASH_1 = 0x5396fdc7460c1b71a4f862f18bf9431c45e20b45aacadb352a014466d4ae66cb;
    bytes32 private constant LEAF_HASH_2 = 0x045f3be9706f4d1a03c5f9bdca61e3c11233328c672e0b665c86cc0a4e0ea974;
    bytes32 private constant LEAF_HASH_3 = 0xd19401f6b5c0fe6c8193dbf7a5a53909f5d8ed6f55356a42e512b0d513d761be;

    bytes32 private constant NODE_01 = 0x0d050aa2b16e952271dc4956798d23e1f2b8b6cd30e8646278b2e84600d02467;
    bytes32 private constant FOUR_LEAF_ROOT = 0x2e45362bb14719212c9600eacf8a181b5f55ca8d422119f7c63b3819c14a3160;
    bytes32 private constant THREE_LEAF_ROOT = 0x40a4c782091c17a1e0f56761e52963c4bd88c1fb0ed4927fc4b270ca21e6008f;

    function setUp() public {
        harness = new MerkleVectorHarness();
    }

    function testLeafHashMatchesNativeVector() public pure {
        require(MerkleLib.hashLeaf(LEAF_VALUE_0) == LEAF_HASH_0, "leaf 0");
        require(MerkleLib.hashLeaf(LEAF_VALUE_1) == LEAF_HASH_1, "leaf 1");
        require(MerkleLib.hashLeaf(LEAF_VALUE_2) == LEAF_HASH_2, "leaf 2");
        require(MerkleLib.hashLeaf(LEAF_VALUE_3) == LEAF_HASH_3, "leaf 3");
    }

    function testInternalNodeHashMatchesNativeVector() public pure {
        require(MerkleLib.hashNode(LEAF_HASH_0, LEAF_HASH_1) == NODE_01, "internal node");
    }

    function testInternalNodeRejectsSupersededNodeDomain() public pure {
        bytes32 superseded = sha256(abi.encodePacked("LXP/v1/merkle-node\x00", LEAF_HASH_0, LEAF_HASH_1));
        require(MerkleLib.hashNode(LEAF_HASH_0, LEAF_HASH_1) != superseded, "superseded domain");
    }

    function testFourLeafProofReproducesNativeRoot() public view {
        bytes32[] memory siblings = new bytes32[](2);
        siblings[0] = LEAF_HASH_3;
        siblings[1] = NODE_01;
        require(harness.proofRoot(LEAF_HASH_2, 2, siblings) == FOUR_LEAF_ROOT, "four leaf root");
    }

    function testThreeLeafProofReproducesNativeRoot() public view {
        bytes32[] memory siblings = new bytes32[](2);
        siblings[0] = LEAF_HASH_2;
        siblings[1] = NODE_01;
        require(harness.proofRoot(LEAF_HASH_2, 2, siblings) == THREE_LEAF_ROOT, "three leaf root");
    }
}
