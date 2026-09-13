// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.30;

library ArchiveSha256 {
    struct State {
        bytes32 words;
        bytes tail;
    }

    function initialize(State storage state) internal {
        state.words = 0x6a09e667bb67ae853c6ef372a54ff53a510e527f9b05688c1f83d9ab5be0cd19;
    }

    function update(State storage state, bytes calldata value) internal {
        bytes memory input = bytes.concat(state.tail, value);
        uint256 blocks = input.length / 64;
        state.words = compress(state.words, input, blocks);
        bytes memory tail = new bytes(input.length % 64);
        for (uint256 i = 0; i < tail.length; ++i) tail[i] = input[blocks * 64 + i];
        state.tail = tail;
    }

    function digest(State storage state, uint64 totalBytes) internal view returns (bytes32) {
        bytes memory tail = state.tail;
        assert(tail.length == totalBytes % 64);
        bytes memory padded = new bytes(tail.length < 56 ? 64 : 128);
        for (uint256 i = 0; i < tail.length; ++i) padded[i] = tail[i];
        padded[tail.length] = 0x80;
        uint64 bits = totalBytes * 8;
        for (uint256 i = 0; i < 8; ++i) padded[padded.length - 1 - i] = bytes1(uint8(bits >> (8 * i)));
        return compress(state.words, padded, padded.length / 64);
    }

    function compress(bytes32 packed, bytes memory input, uint256 blocks) private pure returns (bytes32 result) {
        bytes memory constants = hex"428a2f9871374491b5c0fbcfe9b5dba53956c25b59f111f1923f82a4ab1c5ed5d807aa9812835b01243185be550c7dc372be5d7480deb1fe9bdc06a7c19bf174e49b69c1efbe47860fc19dc6240ca1cc2de92c6f4a7484aa5cb0a9dc76f988da983e5152a831c66db00327c8bf597fc7c6e00bf3d5a7914706ca63511429296727b70a852e1b21384d2c6dfc53380d13650a7354766a0abb81c2c92e92722c85a2bfe8a1a81a664bc24b8b70c76c51a3d192e819d6990624f40e3585106aa07019a4c1161e376c082748774c34b0bcb5391c0cb34ed8aa4a5b9cca4f682e6ff3748f82ee78a5636f84c878148cc7020890befffaa4506cebbef9a3f7c67178f2";
        uint32[8] memory state;
        uint32[64] memory words;
        assembly ("memory-safe") {
            function rotate(value, bits) -> rotated {
                rotated := and(or(shr(bits, value), shl(sub(32, bits), value)), 0xffffffff)
            }
            function transform(statePointer, wordPointer, constantPointer, blockPointer) {
                for { let i := 0 } lt(i, 16) { i := add(i, 1) } {
                    let group := mload(add(blockPointer, mul(div(i, 8), 32)))
                    mstore(add(wordPointer, mul(i, 32)), and(shr(sub(224, mul(mod(i, 8), 32)), group), 0xffffffff))
                }
                for { let i := 16 } lt(i, 64) { i := add(i, 1) } {
                    let x := mload(add(wordPointer, mul(sub(i, 15), 32)))
                    let y := mload(add(wordPointer, mul(sub(i, 2), 32)))
                    let s0 := xor(xor(rotate(x, 7), rotate(x, 18)), shr(3, x))
                    let s1 := xor(xor(rotate(y, 17), rotate(y, 19)), shr(10, y))
                    let sum := add(add(mload(add(wordPointer, mul(sub(i, 16), 32))), s0), add(mload(add(wordPointer, mul(sub(i, 7), 32))), s1))
                    mstore(add(wordPointer, mul(i, 32)), and(sum, 0xffffffff))
                }
                let a := mload(statePointer)
                let b := mload(add(statePointer, 32))
                let c := mload(add(statePointer, 64))
                let d := mload(add(statePointer, 96))
                let e := mload(add(statePointer, 128))
                let f := mload(add(statePointer, 160))
                let g := mload(add(statePointer, 192))
                let h := mload(add(statePointer, 224))
                for { let i := 0 } lt(i, 64) { i := add(i, 1) } {
                    let s1 := xor(xor(rotate(e, 6), rotate(e, 11)), rotate(e, 25))
                    let choice := xor(and(e, f), and(not(e), g))
                    let first := and(add(add(add(h, s1), choice), add(and(shr(sub(224, mul(mod(i, 8), 32)), mload(add(constantPointer, mul(div(i, 8), 32)))), 0xffffffff), mload(add(wordPointer, mul(i, 32))))), 0xffffffff)
                    let s0 := xor(xor(rotate(a, 2), rotate(a, 13)), rotate(a, 22))
                    let majority := xor(xor(and(a, b), and(a, c)), and(b, c))
                    let second := and(add(s0, majority), 0xffffffff)
                    h := g
                    g := f
                    f := e
                    e := and(add(d, first), 0xffffffff)
                    d := c
                    c := b
                    b := a
                    a := and(add(first, second), 0xffffffff)
                }
                mstore(statePointer, and(add(mload(statePointer), a), 0xffffffff))
                mstore(add(statePointer, 32), and(add(mload(add(statePointer, 32)), b), 0xffffffff))
                mstore(add(statePointer, 64), and(add(mload(add(statePointer, 64)), c), 0xffffffff))
                mstore(add(statePointer, 96), and(add(mload(add(statePointer, 96)), d), 0xffffffff))
                mstore(add(statePointer, 128), and(add(mload(add(statePointer, 128)), e), 0xffffffff))
                mstore(add(statePointer, 160), and(add(mload(add(statePointer, 160)), f), 0xffffffff))
                mstore(add(statePointer, 192), and(add(mload(add(statePointer, 192)), g), 0xffffffff))
                mstore(add(statePointer, 224), and(add(mload(add(statePointer, 224)), h), 0xffffffff))
            }
            for { let i := 0 } lt(i, 8) { i := add(i, 1) } {
                mstore(add(state, mul(i, 32)), and(shr(sub(224, mul(i, 32)), packed), 0xffffffff))
            }
            for { let i := 0 } lt(i, blocks) { i := add(i, 1) } {
                transform(state, words, add(constants, 32), add(add(input, 32), mul(i, 64)))
            }
            for { let i := 0 } lt(i, 8) { i := add(i, 1) } {
                result := or(result, shl(sub(224, mul(i, 32)), mload(add(state, mul(i, 32)))))
            }
        }
    }
}
