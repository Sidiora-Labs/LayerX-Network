// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

library Types {
    enum Rounding {
        Down,
        Up
    }

    struct Signature {
        bytes32 r;
        bytes32 s;
        uint8 v;
    }

    struct MerkleProof {
        uint256 leafIndex;
        bytes32[] siblings;
    }

    struct DecimalConversion {
        uint256 converted;
        uint256 remainder;
    }

    struct CallCommitment {
        address target;
        uint256 value;
        bytes32 dataHash;
        bytes32 expectedCodeHashBefore;
        bytes32 expectedCodeHashAfter;
    }
}
