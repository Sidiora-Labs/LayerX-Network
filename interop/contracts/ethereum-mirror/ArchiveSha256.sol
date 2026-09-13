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
        uint32[8] memory state;
        uint32[16] memory words;
        assembly ("memory-safe") {
            function transform(statePointer, wordPointer, blockPointer) {
                {
                    let firstWords := mload(blockPointer)
                    let lastWords := mload(add(blockPointer, 32))
                    mstore(add(wordPointer, 0), and(shr(224, firstWords), 0xffffffff))
                    mstore(add(wordPointer, 32), and(shr(192, firstWords), 0xffffffff))
                    mstore(add(wordPointer, 64), and(shr(160, firstWords), 0xffffffff))
                    mstore(add(wordPointer, 96), and(shr(128, firstWords), 0xffffffff))
                    mstore(add(wordPointer, 128), and(shr(96, firstWords), 0xffffffff))
                    mstore(add(wordPointer, 160), and(shr(64, firstWords), 0xffffffff))
                    mstore(add(wordPointer, 192), and(shr(32, firstWords), 0xffffffff))
                    mstore(add(wordPointer, 224), and(shr(0, firstWords), 0xffffffff))
                    mstore(add(wordPointer, 256), and(shr(224, lastWords), 0xffffffff))
                    mstore(add(wordPointer, 288), and(shr(192, lastWords), 0xffffffff))
                    mstore(add(wordPointer, 320), and(shr(160, lastWords), 0xffffffff))
                    mstore(add(wordPointer, 352), and(shr(128, lastWords), 0xffffffff))
                    mstore(add(wordPointer, 384), and(shr(96, lastWords), 0xffffffff))
                    mstore(add(wordPointer, 416), and(shr(64, lastWords), 0xffffffff))
                    mstore(add(wordPointer, 448), and(shr(32, lastWords), 0xffffffff))
                    mstore(add(wordPointer, 480), and(shr(0, lastWords), 0xffffffff))
                }
                let a := mload(add(statePointer, 0))
                let b := mload(add(statePointer, 32))
                let c := mload(add(statePointer, 64))
                let d := mload(add(statePointer, 96))
                let e := mload(add(statePointer, 128))
                let f := mload(add(statePointer, 160))
                let g := mload(add(statePointer, 192))
                let h := mload(add(statePointer, 224))
                {
                    let first := and(add(add(add(h, xor(xor(or(shr(6, e), shl(26, e)), or(shr(11, e), shl(21, e))), or(shr(25, e), shl(7, e)))), xor(and(e, f), and(not(e), g))), add(0x428a2f98, mload(add(wordPointer, 0)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, a), shl(30, a)), or(shr(13, a), shl(19, a))), or(shr(22, a), shl(10, a))), xor(xor(and(a, b), and(a, c)), and(b, c))), 0xffffffff)
                    d := and(add(d, first), 0xffffffff)
                    h := and(add(first, second), 0xffffffff)
                }
                {
                    let first := and(add(add(add(g, xor(xor(or(shr(6, d), shl(26, d)), or(shr(11, d), shl(21, d))), or(shr(25, d), shl(7, d)))), xor(and(d, e), and(not(d), f))), add(0x71374491, mload(add(wordPointer, 32)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, h), shl(30, h)), or(shr(13, h), shl(19, h))), or(shr(22, h), shl(10, h))), xor(xor(and(h, a), and(h, b)), and(a, b))), 0xffffffff)
                    c := and(add(c, first), 0xffffffff)
                    g := and(add(first, second), 0xffffffff)
                }
                {
                    let first := and(add(add(add(f, xor(xor(or(shr(6, c), shl(26, c)), or(shr(11, c), shl(21, c))), or(shr(25, c), shl(7, c)))), xor(and(c, d), and(not(c), e))), add(0xb5c0fbcf, mload(add(wordPointer, 64)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, g), shl(30, g)), or(shr(13, g), shl(19, g))), or(shr(22, g), shl(10, g))), xor(xor(and(g, h), and(g, a)), and(h, a))), 0xffffffff)
                    b := and(add(b, first), 0xffffffff)
                    f := and(add(first, second), 0xffffffff)
                }
                {
                    let first := and(add(add(add(e, xor(xor(or(shr(6, b), shl(26, b)), or(shr(11, b), shl(21, b))), or(shr(25, b), shl(7, b)))), xor(and(b, c), and(not(b), d))), add(0xe9b5dba5, mload(add(wordPointer, 96)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, f), shl(30, f)), or(shr(13, f), shl(19, f))), or(shr(22, f), shl(10, f))), xor(xor(and(f, g), and(f, h)), and(g, h))), 0xffffffff)
                    a := and(add(a, first), 0xffffffff)
                    e := and(add(first, second), 0xffffffff)
                }
                {
                    let first := and(add(add(add(d, xor(xor(or(shr(6, a), shl(26, a)), or(shr(11, a), shl(21, a))), or(shr(25, a), shl(7, a)))), xor(and(a, b), and(not(a), c))), add(0x3956c25b, mload(add(wordPointer, 128)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, e), shl(30, e)), or(shr(13, e), shl(19, e))), or(shr(22, e), shl(10, e))), xor(xor(and(e, f), and(e, g)), and(f, g))), 0xffffffff)
                    h := and(add(h, first), 0xffffffff)
                    d := and(add(first, second), 0xffffffff)
                }
                {
                    let first := and(add(add(add(c, xor(xor(or(shr(6, h), shl(26, h)), or(shr(11, h), shl(21, h))), or(shr(25, h), shl(7, h)))), xor(and(h, a), and(not(h), b))), add(0x59f111f1, mload(add(wordPointer, 160)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, d), shl(30, d)), or(shr(13, d), shl(19, d))), or(shr(22, d), shl(10, d))), xor(xor(and(d, e), and(d, f)), and(e, f))), 0xffffffff)
                    g := and(add(g, first), 0xffffffff)
                    c := and(add(first, second), 0xffffffff)
                }
                {
                    let first := and(add(add(add(b, xor(xor(or(shr(6, g), shl(26, g)), or(shr(11, g), shl(21, g))), or(shr(25, g), shl(7, g)))), xor(and(g, h), and(not(g), a))), add(0x923f82a4, mload(add(wordPointer, 192)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, c), shl(30, c)), or(shr(13, c), shl(19, c))), or(shr(22, c), shl(10, c))), xor(xor(and(c, d), and(c, e)), and(d, e))), 0xffffffff)
                    f := and(add(f, first), 0xffffffff)
                    b := and(add(first, second), 0xffffffff)
                }
                {
                    let first := and(add(add(add(a, xor(xor(or(shr(6, f), shl(26, f)), or(shr(11, f), shl(21, f))), or(shr(25, f), shl(7, f)))), xor(and(f, g), and(not(f), h))), add(0xab1c5ed5, mload(add(wordPointer, 224)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, b), shl(30, b)), or(shr(13, b), shl(19, b))), or(shr(22, b), shl(10, b))), xor(xor(and(b, c), and(b, d)), and(c, d))), 0xffffffff)
                    e := and(add(e, first), 0xffffffff)
                    a := and(add(first, second), 0xffffffff)
                }
                {
                    let first := and(add(add(add(h, xor(xor(or(shr(6, e), shl(26, e)), or(shr(11, e), shl(21, e))), or(shr(25, e), shl(7, e)))), xor(and(e, f), and(not(e), g))), add(0xd807aa98, mload(add(wordPointer, 256)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, a), shl(30, a)), or(shr(13, a), shl(19, a))), or(shr(22, a), shl(10, a))), xor(xor(and(a, b), and(a, c)), and(b, c))), 0xffffffff)
                    d := and(add(d, first), 0xffffffff)
                    h := and(add(first, second), 0xffffffff)
                }
                {
                    let first := and(add(add(add(g, xor(xor(or(shr(6, d), shl(26, d)), or(shr(11, d), shl(21, d))), or(shr(25, d), shl(7, d)))), xor(and(d, e), and(not(d), f))), add(0x12835b01, mload(add(wordPointer, 288)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, h), shl(30, h)), or(shr(13, h), shl(19, h))), or(shr(22, h), shl(10, h))), xor(xor(and(h, a), and(h, b)), and(a, b))), 0xffffffff)
                    c := and(add(c, first), 0xffffffff)
                    g := and(add(first, second), 0xffffffff)
                }
                {
                    let first := and(add(add(add(f, xor(xor(or(shr(6, c), shl(26, c)), or(shr(11, c), shl(21, c))), or(shr(25, c), shl(7, c)))), xor(and(c, d), and(not(c), e))), add(0x243185be, mload(add(wordPointer, 320)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, g), shl(30, g)), or(shr(13, g), shl(19, g))), or(shr(22, g), shl(10, g))), xor(xor(and(g, h), and(g, a)), and(h, a))), 0xffffffff)
                    b := and(add(b, first), 0xffffffff)
                    f := and(add(first, second), 0xffffffff)
                }
                {
                    let first := and(add(add(add(e, xor(xor(or(shr(6, b), shl(26, b)), or(shr(11, b), shl(21, b))), or(shr(25, b), shl(7, b)))), xor(and(b, c), and(not(b), d))), add(0x550c7dc3, mload(add(wordPointer, 352)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, f), shl(30, f)), or(shr(13, f), shl(19, f))), or(shr(22, f), shl(10, f))), xor(xor(and(f, g), and(f, h)), and(g, h))), 0xffffffff)
                    a := and(add(a, first), 0xffffffff)
                    e := and(add(first, second), 0xffffffff)
                }
                {
                    let first := and(add(add(add(d, xor(xor(or(shr(6, a), shl(26, a)), or(shr(11, a), shl(21, a))), or(shr(25, a), shl(7, a)))), xor(and(a, b), and(not(a), c))), add(0x72be5d74, mload(add(wordPointer, 384)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, e), shl(30, e)), or(shr(13, e), shl(19, e))), or(shr(22, e), shl(10, e))), xor(xor(and(e, f), and(e, g)), and(f, g))), 0xffffffff)
                    h := and(add(h, first), 0xffffffff)
                    d := and(add(first, second), 0xffffffff)
                }
                {
                    let first := and(add(add(add(c, xor(xor(or(shr(6, h), shl(26, h)), or(shr(11, h), shl(21, h))), or(shr(25, h), shl(7, h)))), xor(and(h, a), and(not(h), b))), add(0x80deb1fe, mload(add(wordPointer, 416)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, d), shl(30, d)), or(shr(13, d), shl(19, d))), or(shr(22, d), shl(10, d))), xor(xor(and(d, e), and(d, f)), and(e, f))), 0xffffffff)
                    g := and(add(g, first), 0xffffffff)
                    c := and(add(first, second), 0xffffffff)
                }
                {
                    let first := and(add(add(add(b, xor(xor(or(shr(6, g), shl(26, g)), or(shr(11, g), shl(21, g))), or(shr(25, g), shl(7, g)))), xor(and(g, h), and(not(g), a))), add(0x9bdc06a7, mload(add(wordPointer, 448)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, c), shl(30, c)), or(shr(13, c), shl(19, c))), or(shr(22, c), shl(10, c))), xor(xor(and(c, d), and(c, e)), and(d, e))), 0xffffffff)
                    f := and(add(f, first), 0xffffffff)
                    b := and(add(first, second), 0xffffffff)
                }
                {
                    let first := and(add(add(add(a, xor(xor(or(shr(6, f), shl(26, f)), or(shr(11, f), shl(21, f))), or(shr(25, f), shl(7, f)))), xor(and(f, g), and(not(f), h))), add(0xc19bf174, mload(add(wordPointer, 480)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, b), shl(30, b)), or(shr(13, b), shl(19, b))), or(shr(22, b), shl(10, b))), xor(xor(and(b, c), and(b, d)), and(c, d))), 0xffffffff)
                    e := and(add(e, first), 0xffffffff)
                    a := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 32))
                    let y := mload(add(wordPointer, 448))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 0), and(add(add(mload(add(wordPointer, 0)), s0), add(mload(add(wordPointer, 288)), s1)), 0xffffffff))
                    let first := and(add(add(add(h, xor(xor(or(shr(6, e), shl(26, e)), or(shr(11, e), shl(21, e))), or(shr(25, e), shl(7, e)))), xor(and(e, f), and(not(e), g))), add(0xe49b69c1, mload(add(wordPointer, 0)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, a), shl(30, a)), or(shr(13, a), shl(19, a))), or(shr(22, a), shl(10, a))), xor(xor(and(a, b), and(a, c)), and(b, c))), 0xffffffff)
                    d := and(add(d, first), 0xffffffff)
                    h := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 64))
                    let y := mload(add(wordPointer, 480))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 32), and(add(add(mload(add(wordPointer, 32)), s0), add(mload(add(wordPointer, 320)), s1)), 0xffffffff))
                    let first := and(add(add(add(g, xor(xor(or(shr(6, d), shl(26, d)), or(shr(11, d), shl(21, d))), or(shr(25, d), shl(7, d)))), xor(and(d, e), and(not(d), f))), add(0xefbe4786, mload(add(wordPointer, 32)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, h), shl(30, h)), or(shr(13, h), shl(19, h))), or(shr(22, h), shl(10, h))), xor(xor(and(h, a), and(h, b)), and(a, b))), 0xffffffff)
                    c := and(add(c, first), 0xffffffff)
                    g := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 96))
                    let y := mload(add(wordPointer, 0))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 64), and(add(add(mload(add(wordPointer, 64)), s0), add(mload(add(wordPointer, 352)), s1)), 0xffffffff))
                    let first := and(add(add(add(f, xor(xor(or(shr(6, c), shl(26, c)), or(shr(11, c), shl(21, c))), or(shr(25, c), shl(7, c)))), xor(and(c, d), and(not(c), e))), add(0x0fc19dc6, mload(add(wordPointer, 64)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, g), shl(30, g)), or(shr(13, g), shl(19, g))), or(shr(22, g), shl(10, g))), xor(xor(and(g, h), and(g, a)), and(h, a))), 0xffffffff)
                    b := and(add(b, first), 0xffffffff)
                    f := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 128))
                    let y := mload(add(wordPointer, 32))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 96), and(add(add(mload(add(wordPointer, 96)), s0), add(mload(add(wordPointer, 384)), s1)), 0xffffffff))
                    let first := and(add(add(add(e, xor(xor(or(shr(6, b), shl(26, b)), or(shr(11, b), shl(21, b))), or(shr(25, b), shl(7, b)))), xor(and(b, c), and(not(b), d))), add(0x240ca1cc, mload(add(wordPointer, 96)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, f), shl(30, f)), or(shr(13, f), shl(19, f))), or(shr(22, f), shl(10, f))), xor(xor(and(f, g), and(f, h)), and(g, h))), 0xffffffff)
                    a := and(add(a, first), 0xffffffff)
                    e := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 160))
                    let y := mload(add(wordPointer, 64))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 128), and(add(add(mload(add(wordPointer, 128)), s0), add(mload(add(wordPointer, 416)), s1)), 0xffffffff))
                    let first := and(add(add(add(d, xor(xor(or(shr(6, a), shl(26, a)), or(shr(11, a), shl(21, a))), or(shr(25, a), shl(7, a)))), xor(and(a, b), and(not(a), c))), add(0x2de92c6f, mload(add(wordPointer, 128)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, e), shl(30, e)), or(shr(13, e), shl(19, e))), or(shr(22, e), shl(10, e))), xor(xor(and(e, f), and(e, g)), and(f, g))), 0xffffffff)
                    h := and(add(h, first), 0xffffffff)
                    d := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 192))
                    let y := mload(add(wordPointer, 96))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 160), and(add(add(mload(add(wordPointer, 160)), s0), add(mload(add(wordPointer, 448)), s1)), 0xffffffff))
                    let first := and(add(add(add(c, xor(xor(or(shr(6, h), shl(26, h)), or(shr(11, h), shl(21, h))), or(shr(25, h), shl(7, h)))), xor(and(h, a), and(not(h), b))), add(0x4a7484aa, mload(add(wordPointer, 160)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, d), shl(30, d)), or(shr(13, d), shl(19, d))), or(shr(22, d), shl(10, d))), xor(xor(and(d, e), and(d, f)), and(e, f))), 0xffffffff)
                    g := and(add(g, first), 0xffffffff)
                    c := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 224))
                    let y := mload(add(wordPointer, 128))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 192), and(add(add(mload(add(wordPointer, 192)), s0), add(mload(add(wordPointer, 480)), s1)), 0xffffffff))
                    let first := and(add(add(add(b, xor(xor(or(shr(6, g), shl(26, g)), or(shr(11, g), shl(21, g))), or(shr(25, g), shl(7, g)))), xor(and(g, h), and(not(g), a))), add(0x5cb0a9dc, mload(add(wordPointer, 192)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, c), shl(30, c)), or(shr(13, c), shl(19, c))), or(shr(22, c), shl(10, c))), xor(xor(and(c, d), and(c, e)), and(d, e))), 0xffffffff)
                    f := and(add(f, first), 0xffffffff)
                    b := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 256))
                    let y := mload(add(wordPointer, 160))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 224), and(add(add(mload(add(wordPointer, 224)), s0), add(mload(add(wordPointer, 0)), s1)), 0xffffffff))
                    let first := and(add(add(add(a, xor(xor(or(shr(6, f), shl(26, f)), or(shr(11, f), shl(21, f))), or(shr(25, f), shl(7, f)))), xor(and(f, g), and(not(f), h))), add(0x76f988da, mload(add(wordPointer, 224)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, b), shl(30, b)), or(shr(13, b), shl(19, b))), or(shr(22, b), shl(10, b))), xor(xor(and(b, c), and(b, d)), and(c, d))), 0xffffffff)
                    e := and(add(e, first), 0xffffffff)
                    a := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 288))
                    let y := mload(add(wordPointer, 192))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 256), and(add(add(mload(add(wordPointer, 256)), s0), add(mload(add(wordPointer, 32)), s1)), 0xffffffff))
                    let first := and(add(add(add(h, xor(xor(or(shr(6, e), shl(26, e)), or(shr(11, e), shl(21, e))), or(shr(25, e), shl(7, e)))), xor(and(e, f), and(not(e), g))), add(0x983e5152, mload(add(wordPointer, 256)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, a), shl(30, a)), or(shr(13, a), shl(19, a))), or(shr(22, a), shl(10, a))), xor(xor(and(a, b), and(a, c)), and(b, c))), 0xffffffff)
                    d := and(add(d, first), 0xffffffff)
                    h := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 320))
                    let y := mload(add(wordPointer, 224))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 288), and(add(add(mload(add(wordPointer, 288)), s0), add(mload(add(wordPointer, 64)), s1)), 0xffffffff))
                    let first := and(add(add(add(g, xor(xor(or(shr(6, d), shl(26, d)), or(shr(11, d), shl(21, d))), or(shr(25, d), shl(7, d)))), xor(and(d, e), and(not(d), f))), add(0xa831c66d, mload(add(wordPointer, 288)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, h), shl(30, h)), or(shr(13, h), shl(19, h))), or(shr(22, h), shl(10, h))), xor(xor(and(h, a), and(h, b)), and(a, b))), 0xffffffff)
                    c := and(add(c, first), 0xffffffff)
                    g := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 352))
                    let y := mload(add(wordPointer, 256))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 320), and(add(add(mload(add(wordPointer, 320)), s0), add(mload(add(wordPointer, 96)), s1)), 0xffffffff))
                    let first := and(add(add(add(f, xor(xor(or(shr(6, c), shl(26, c)), or(shr(11, c), shl(21, c))), or(shr(25, c), shl(7, c)))), xor(and(c, d), and(not(c), e))), add(0xb00327c8, mload(add(wordPointer, 320)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, g), shl(30, g)), or(shr(13, g), shl(19, g))), or(shr(22, g), shl(10, g))), xor(xor(and(g, h), and(g, a)), and(h, a))), 0xffffffff)
                    b := and(add(b, first), 0xffffffff)
                    f := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 384))
                    let y := mload(add(wordPointer, 288))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 352), and(add(add(mload(add(wordPointer, 352)), s0), add(mload(add(wordPointer, 128)), s1)), 0xffffffff))
                    let first := and(add(add(add(e, xor(xor(or(shr(6, b), shl(26, b)), or(shr(11, b), shl(21, b))), or(shr(25, b), shl(7, b)))), xor(and(b, c), and(not(b), d))), add(0xbf597fc7, mload(add(wordPointer, 352)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, f), shl(30, f)), or(shr(13, f), shl(19, f))), or(shr(22, f), shl(10, f))), xor(xor(and(f, g), and(f, h)), and(g, h))), 0xffffffff)
                    a := and(add(a, first), 0xffffffff)
                    e := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 416))
                    let y := mload(add(wordPointer, 320))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 384), and(add(add(mload(add(wordPointer, 384)), s0), add(mload(add(wordPointer, 160)), s1)), 0xffffffff))
                    let first := and(add(add(add(d, xor(xor(or(shr(6, a), shl(26, a)), or(shr(11, a), shl(21, a))), or(shr(25, a), shl(7, a)))), xor(and(a, b), and(not(a), c))), add(0xc6e00bf3, mload(add(wordPointer, 384)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, e), shl(30, e)), or(shr(13, e), shl(19, e))), or(shr(22, e), shl(10, e))), xor(xor(and(e, f), and(e, g)), and(f, g))), 0xffffffff)
                    h := and(add(h, first), 0xffffffff)
                    d := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 448))
                    let y := mload(add(wordPointer, 352))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 416), and(add(add(mload(add(wordPointer, 416)), s0), add(mload(add(wordPointer, 192)), s1)), 0xffffffff))
                    let first := and(add(add(add(c, xor(xor(or(shr(6, h), shl(26, h)), or(shr(11, h), shl(21, h))), or(shr(25, h), shl(7, h)))), xor(and(h, a), and(not(h), b))), add(0xd5a79147, mload(add(wordPointer, 416)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, d), shl(30, d)), or(shr(13, d), shl(19, d))), or(shr(22, d), shl(10, d))), xor(xor(and(d, e), and(d, f)), and(e, f))), 0xffffffff)
                    g := and(add(g, first), 0xffffffff)
                    c := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 480))
                    let y := mload(add(wordPointer, 384))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 448), and(add(add(mload(add(wordPointer, 448)), s0), add(mload(add(wordPointer, 224)), s1)), 0xffffffff))
                    let first := and(add(add(add(b, xor(xor(or(shr(6, g), shl(26, g)), or(shr(11, g), shl(21, g))), or(shr(25, g), shl(7, g)))), xor(and(g, h), and(not(g), a))), add(0x06ca6351, mload(add(wordPointer, 448)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, c), shl(30, c)), or(shr(13, c), shl(19, c))), or(shr(22, c), shl(10, c))), xor(xor(and(c, d), and(c, e)), and(d, e))), 0xffffffff)
                    f := and(add(f, first), 0xffffffff)
                    b := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 0))
                    let y := mload(add(wordPointer, 416))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 480), and(add(add(mload(add(wordPointer, 480)), s0), add(mload(add(wordPointer, 256)), s1)), 0xffffffff))
                    let first := and(add(add(add(a, xor(xor(or(shr(6, f), shl(26, f)), or(shr(11, f), shl(21, f))), or(shr(25, f), shl(7, f)))), xor(and(f, g), and(not(f), h))), add(0x14292967, mload(add(wordPointer, 480)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, b), shl(30, b)), or(shr(13, b), shl(19, b))), or(shr(22, b), shl(10, b))), xor(xor(and(b, c), and(b, d)), and(c, d))), 0xffffffff)
                    e := and(add(e, first), 0xffffffff)
                    a := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 32))
                    let y := mload(add(wordPointer, 448))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 0), and(add(add(mload(add(wordPointer, 0)), s0), add(mload(add(wordPointer, 288)), s1)), 0xffffffff))
                    let first := and(add(add(add(h, xor(xor(or(shr(6, e), shl(26, e)), or(shr(11, e), shl(21, e))), or(shr(25, e), shl(7, e)))), xor(and(e, f), and(not(e), g))), add(0x27b70a85, mload(add(wordPointer, 0)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, a), shl(30, a)), or(shr(13, a), shl(19, a))), or(shr(22, a), shl(10, a))), xor(xor(and(a, b), and(a, c)), and(b, c))), 0xffffffff)
                    d := and(add(d, first), 0xffffffff)
                    h := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 64))
                    let y := mload(add(wordPointer, 480))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 32), and(add(add(mload(add(wordPointer, 32)), s0), add(mload(add(wordPointer, 320)), s1)), 0xffffffff))
                    let first := and(add(add(add(g, xor(xor(or(shr(6, d), shl(26, d)), or(shr(11, d), shl(21, d))), or(shr(25, d), shl(7, d)))), xor(and(d, e), and(not(d), f))), add(0x2e1b2138, mload(add(wordPointer, 32)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, h), shl(30, h)), or(shr(13, h), shl(19, h))), or(shr(22, h), shl(10, h))), xor(xor(and(h, a), and(h, b)), and(a, b))), 0xffffffff)
                    c := and(add(c, first), 0xffffffff)
                    g := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 96))
                    let y := mload(add(wordPointer, 0))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 64), and(add(add(mload(add(wordPointer, 64)), s0), add(mload(add(wordPointer, 352)), s1)), 0xffffffff))
                    let first := and(add(add(add(f, xor(xor(or(shr(6, c), shl(26, c)), or(shr(11, c), shl(21, c))), or(shr(25, c), shl(7, c)))), xor(and(c, d), and(not(c), e))), add(0x4d2c6dfc, mload(add(wordPointer, 64)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, g), shl(30, g)), or(shr(13, g), shl(19, g))), or(shr(22, g), shl(10, g))), xor(xor(and(g, h), and(g, a)), and(h, a))), 0xffffffff)
                    b := and(add(b, first), 0xffffffff)
                    f := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 128))
                    let y := mload(add(wordPointer, 32))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 96), and(add(add(mload(add(wordPointer, 96)), s0), add(mload(add(wordPointer, 384)), s1)), 0xffffffff))
                    let first := and(add(add(add(e, xor(xor(or(shr(6, b), shl(26, b)), or(shr(11, b), shl(21, b))), or(shr(25, b), shl(7, b)))), xor(and(b, c), and(not(b), d))), add(0x53380d13, mload(add(wordPointer, 96)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, f), shl(30, f)), or(shr(13, f), shl(19, f))), or(shr(22, f), shl(10, f))), xor(xor(and(f, g), and(f, h)), and(g, h))), 0xffffffff)
                    a := and(add(a, first), 0xffffffff)
                    e := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 160))
                    let y := mload(add(wordPointer, 64))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 128), and(add(add(mload(add(wordPointer, 128)), s0), add(mload(add(wordPointer, 416)), s1)), 0xffffffff))
                    let first := and(add(add(add(d, xor(xor(or(shr(6, a), shl(26, a)), or(shr(11, a), shl(21, a))), or(shr(25, a), shl(7, a)))), xor(and(a, b), and(not(a), c))), add(0x650a7354, mload(add(wordPointer, 128)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, e), shl(30, e)), or(shr(13, e), shl(19, e))), or(shr(22, e), shl(10, e))), xor(xor(and(e, f), and(e, g)), and(f, g))), 0xffffffff)
                    h := and(add(h, first), 0xffffffff)
                    d := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 192))
                    let y := mload(add(wordPointer, 96))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 160), and(add(add(mload(add(wordPointer, 160)), s0), add(mload(add(wordPointer, 448)), s1)), 0xffffffff))
                    let first := and(add(add(add(c, xor(xor(or(shr(6, h), shl(26, h)), or(shr(11, h), shl(21, h))), or(shr(25, h), shl(7, h)))), xor(and(h, a), and(not(h), b))), add(0x766a0abb, mload(add(wordPointer, 160)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, d), shl(30, d)), or(shr(13, d), shl(19, d))), or(shr(22, d), shl(10, d))), xor(xor(and(d, e), and(d, f)), and(e, f))), 0xffffffff)
                    g := and(add(g, first), 0xffffffff)
                    c := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 224))
                    let y := mload(add(wordPointer, 128))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 192), and(add(add(mload(add(wordPointer, 192)), s0), add(mload(add(wordPointer, 480)), s1)), 0xffffffff))
                    let first := and(add(add(add(b, xor(xor(or(shr(6, g), shl(26, g)), or(shr(11, g), shl(21, g))), or(shr(25, g), shl(7, g)))), xor(and(g, h), and(not(g), a))), add(0x81c2c92e, mload(add(wordPointer, 192)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, c), shl(30, c)), or(shr(13, c), shl(19, c))), or(shr(22, c), shl(10, c))), xor(xor(and(c, d), and(c, e)), and(d, e))), 0xffffffff)
                    f := and(add(f, first), 0xffffffff)
                    b := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 256))
                    let y := mload(add(wordPointer, 160))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 224), and(add(add(mload(add(wordPointer, 224)), s0), add(mload(add(wordPointer, 0)), s1)), 0xffffffff))
                    let first := and(add(add(add(a, xor(xor(or(shr(6, f), shl(26, f)), or(shr(11, f), shl(21, f))), or(shr(25, f), shl(7, f)))), xor(and(f, g), and(not(f), h))), add(0x92722c85, mload(add(wordPointer, 224)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, b), shl(30, b)), or(shr(13, b), shl(19, b))), or(shr(22, b), shl(10, b))), xor(xor(and(b, c), and(b, d)), and(c, d))), 0xffffffff)
                    e := and(add(e, first), 0xffffffff)
                    a := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 288))
                    let y := mload(add(wordPointer, 192))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 256), and(add(add(mload(add(wordPointer, 256)), s0), add(mload(add(wordPointer, 32)), s1)), 0xffffffff))
                    let first := and(add(add(add(h, xor(xor(or(shr(6, e), shl(26, e)), or(shr(11, e), shl(21, e))), or(shr(25, e), shl(7, e)))), xor(and(e, f), and(not(e), g))), add(0xa2bfe8a1, mload(add(wordPointer, 256)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, a), shl(30, a)), or(shr(13, a), shl(19, a))), or(shr(22, a), shl(10, a))), xor(xor(and(a, b), and(a, c)), and(b, c))), 0xffffffff)
                    d := and(add(d, first), 0xffffffff)
                    h := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 320))
                    let y := mload(add(wordPointer, 224))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 288), and(add(add(mload(add(wordPointer, 288)), s0), add(mload(add(wordPointer, 64)), s1)), 0xffffffff))
                    let first := and(add(add(add(g, xor(xor(or(shr(6, d), shl(26, d)), or(shr(11, d), shl(21, d))), or(shr(25, d), shl(7, d)))), xor(and(d, e), and(not(d), f))), add(0xa81a664b, mload(add(wordPointer, 288)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, h), shl(30, h)), or(shr(13, h), shl(19, h))), or(shr(22, h), shl(10, h))), xor(xor(and(h, a), and(h, b)), and(a, b))), 0xffffffff)
                    c := and(add(c, first), 0xffffffff)
                    g := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 352))
                    let y := mload(add(wordPointer, 256))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 320), and(add(add(mload(add(wordPointer, 320)), s0), add(mload(add(wordPointer, 96)), s1)), 0xffffffff))
                    let first := and(add(add(add(f, xor(xor(or(shr(6, c), shl(26, c)), or(shr(11, c), shl(21, c))), or(shr(25, c), shl(7, c)))), xor(and(c, d), and(not(c), e))), add(0xc24b8b70, mload(add(wordPointer, 320)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, g), shl(30, g)), or(shr(13, g), shl(19, g))), or(shr(22, g), shl(10, g))), xor(xor(and(g, h), and(g, a)), and(h, a))), 0xffffffff)
                    b := and(add(b, first), 0xffffffff)
                    f := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 384))
                    let y := mload(add(wordPointer, 288))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 352), and(add(add(mload(add(wordPointer, 352)), s0), add(mload(add(wordPointer, 128)), s1)), 0xffffffff))
                    let first := and(add(add(add(e, xor(xor(or(shr(6, b), shl(26, b)), or(shr(11, b), shl(21, b))), or(shr(25, b), shl(7, b)))), xor(and(b, c), and(not(b), d))), add(0xc76c51a3, mload(add(wordPointer, 352)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, f), shl(30, f)), or(shr(13, f), shl(19, f))), or(shr(22, f), shl(10, f))), xor(xor(and(f, g), and(f, h)), and(g, h))), 0xffffffff)
                    a := and(add(a, first), 0xffffffff)
                    e := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 416))
                    let y := mload(add(wordPointer, 320))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 384), and(add(add(mload(add(wordPointer, 384)), s0), add(mload(add(wordPointer, 160)), s1)), 0xffffffff))
                    let first := and(add(add(add(d, xor(xor(or(shr(6, a), shl(26, a)), or(shr(11, a), shl(21, a))), or(shr(25, a), shl(7, a)))), xor(and(a, b), and(not(a), c))), add(0xd192e819, mload(add(wordPointer, 384)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, e), shl(30, e)), or(shr(13, e), shl(19, e))), or(shr(22, e), shl(10, e))), xor(xor(and(e, f), and(e, g)), and(f, g))), 0xffffffff)
                    h := and(add(h, first), 0xffffffff)
                    d := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 448))
                    let y := mload(add(wordPointer, 352))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 416), and(add(add(mload(add(wordPointer, 416)), s0), add(mload(add(wordPointer, 192)), s1)), 0xffffffff))
                    let first := and(add(add(add(c, xor(xor(or(shr(6, h), shl(26, h)), or(shr(11, h), shl(21, h))), or(shr(25, h), shl(7, h)))), xor(and(h, a), and(not(h), b))), add(0xd6990624, mload(add(wordPointer, 416)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, d), shl(30, d)), or(shr(13, d), shl(19, d))), or(shr(22, d), shl(10, d))), xor(xor(and(d, e), and(d, f)), and(e, f))), 0xffffffff)
                    g := and(add(g, first), 0xffffffff)
                    c := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 480))
                    let y := mload(add(wordPointer, 384))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 448), and(add(add(mload(add(wordPointer, 448)), s0), add(mload(add(wordPointer, 224)), s1)), 0xffffffff))
                    let first := and(add(add(add(b, xor(xor(or(shr(6, g), shl(26, g)), or(shr(11, g), shl(21, g))), or(shr(25, g), shl(7, g)))), xor(and(g, h), and(not(g), a))), add(0xf40e3585, mload(add(wordPointer, 448)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, c), shl(30, c)), or(shr(13, c), shl(19, c))), or(shr(22, c), shl(10, c))), xor(xor(and(c, d), and(c, e)), and(d, e))), 0xffffffff)
                    f := and(add(f, first), 0xffffffff)
                    b := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 0))
                    let y := mload(add(wordPointer, 416))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 480), and(add(add(mload(add(wordPointer, 480)), s0), add(mload(add(wordPointer, 256)), s1)), 0xffffffff))
                    let first := and(add(add(add(a, xor(xor(or(shr(6, f), shl(26, f)), or(shr(11, f), shl(21, f))), or(shr(25, f), shl(7, f)))), xor(and(f, g), and(not(f), h))), add(0x106aa070, mload(add(wordPointer, 480)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, b), shl(30, b)), or(shr(13, b), shl(19, b))), or(shr(22, b), shl(10, b))), xor(xor(and(b, c), and(b, d)), and(c, d))), 0xffffffff)
                    e := and(add(e, first), 0xffffffff)
                    a := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 32))
                    let y := mload(add(wordPointer, 448))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 0), and(add(add(mload(add(wordPointer, 0)), s0), add(mload(add(wordPointer, 288)), s1)), 0xffffffff))
                    let first := and(add(add(add(h, xor(xor(or(shr(6, e), shl(26, e)), or(shr(11, e), shl(21, e))), or(shr(25, e), shl(7, e)))), xor(and(e, f), and(not(e), g))), add(0x19a4c116, mload(add(wordPointer, 0)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, a), shl(30, a)), or(shr(13, a), shl(19, a))), or(shr(22, a), shl(10, a))), xor(xor(and(a, b), and(a, c)), and(b, c))), 0xffffffff)
                    d := and(add(d, first), 0xffffffff)
                    h := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 64))
                    let y := mload(add(wordPointer, 480))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 32), and(add(add(mload(add(wordPointer, 32)), s0), add(mload(add(wordPointer, 320)), s1)), 0xffffffff))
                    let first := and(add(add(add(g, xor(xor(or(shr(6, d), shl(26, d)), or(shr(11, d), shl(21, d))), or(shr(25, d), shl(7, d)))), xor(and(d, e), and(not(d), f))), add(0x1e376c08, mload(add(wordPointer, 32)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, h), shl(30, h)), or(shr(13, h), shl(19, h))), or(shr(22, h), shl(10, h))), xor(xor(and(h, a), and(h, b)), and(a, b))), 0xffffffff)
                    c := and(add(c, first), 0xffffffff)
                    g := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 96))
                    let y := mload(add(wordPointer, 0))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 64), and(add(add(mload(add(wordPointer, 64)), s0), add(mload(add(wordPointer, 352)), s1)), 0xffffffff))
                    let first := and(add(add(add(f, xor(xor(or(shr(6, c), shl(26, c)), or(shr(11, c), shl(21, c))), or(shr(25, c), shl(7, c)))), xor(and(c, d), and(not(c), e))), add(0x2748774c, mload(add(wordPointer, 64)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, g), shl(30, g)), or(shr(13, g), shl(19, g))), or(shr(22, g), shl(10, g))), xor(xor(and(g, h), and(g, a)), and(h, a))), 0xffffffff)
                    b := and(add(b, first), 0xffffffff)
                    f := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 128))
                    let y := mload(add(wordPointer, 32))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 96), and(add(add(mload(add(wordPointer, 96)), s0), add(mload(add(wordPointer, 384)), s1)), 0xffffffff))
                    let first := and(add(add(add(e, xor(xor(or(shr(6, b), shl(26, b)), or(shr(11, b), shl(21, b))), or(shr(25, b), shl(7, b)))), xor(and(b, c), and(not(b), d))), add(0x34b0bcb5, mload(add(wordPointer, 96)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, f), shl(30, f)), or(shr(13, f), shl(19, f))), or(shr(22, f), shl(10, f))), xor(xor(and(f, g), and(f, h)), and(g, h))), 0xffffffff)
                    a := and(add(a, first), 0xffffffff)
                    e := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 160))
                    let y := mload(add(wordPointer, 64))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 128), and(add(add(mload(add(wordPointer, 128)), s0), add(mload(add(wordPointer, 416)), s1)), 0xffffffff))
                    let first := and(add(add(add(d, xor(xor(or(shr(6, a), shl(26, a)), or(shr(11, a), shl(21, a))), or(shr(25, a), shl(7, a)))), xor(and(a, b), and(not(a), c))), add(0x391c0cb3, mload(add(wordPointer, 128)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, e), shl(30, e)), or(shr(13, e), shl(19, e))), or(shr(22, e), shl(10, e))), xor(xor(and(e, f), and(e, g)), and(f, g))), 0xffffffff)
                    h := and(add(h, first), 0xffffffff)
                    d := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 192))
                    let y := mload(add(wordPointer, 96))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 160), and(add(add(mload(add(wordPointer, 160)), s0), add(mload(add(wordPointer, 448)), s1)), 0xffffffff))
                    let first := and(add(add(add(c, xor(xor(or(shr(6, h), shl(26, h)), or(shr(11, h), shl(21, h))), or(shr(25, h), shl(7, h)))), xor(and(h, a), and(not(h), b))), add(0x4ed8aa4a, mload(add(wordPointer, 160)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, d), shl(30, d)), or(shr(13, d), shl(19, d))), or(shr(22, d), shl(10, d))), xor(xor(and(d, e), and(d, f)), and(e, f))), 0xffffffff)
                    g := and(add(g, first), 0xffffffff)
                    c := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 224))
                    let y := mload(add(wordPointer, 128))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 192), and(add(add(mload(add(wordPointer, 192)), s0), add(mload(add(wordPointer, 480)), s1)), 0xffffffff))
                    let first := and(add(add(add(b, xor(xor(or(shr(6, g), shl(26, g)), or(shr(11, g), shl(21, g))), or(shr(25, g), shl(7, g)))), xor(and(g, h), and(not(g), a))), add(0x5b9cca4f, mload(add(wordPointer, 192)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, c), shl(30, c)), or(shr(13, c), shl(19, c))), or(shr(22, c), shl(10, c))), xor(xor(and(c, d), and(c, e)), and(d, e))), 0xffffffff)
                    f := and(add(f, first), 0xffffffff)
                    b := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 256))
                    let y := mload(add(wordPointer, 160))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 224), and(add(add(mload(add(wordPointer, 224)), s0), add(mload(add(wordPointer, 0)), s1)), 0xffffffff))
                    let first := and(add(add(add(a, xor(xor(or(shr(6, f), shl(26, f)), or(shr(11, f), shl(21, f))), or(shr(25, f), shl(7, f)))), xor(and(f, g), and(not(f), h))), add(0x682e6ff3, mload(add(wordPointer, 224)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, b), shl(30, b)), or(shr(13, b), shl(19, b))), or(shr(22, b), shl(10, b))), xor(xor(and(b, c), and(b, d)), and(c, d))), 0xffffffff)
                    e := and(add(e, first), 0xffffffff)
                    a := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 288))
                    let y := mload(add(wordPointer, 192))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 256), and(add(add(mload(add(wordPointer, 256)), s0), add(mload(add(wordPointer, 32)), s1)), 0xffffffff))
                    let first := and(add(add(add(h, xor(xor(or(shr(6, e), shl(26, e)), or(shr(11, e), shl(21, e))), or(shr(25, e), shl(7, e)))), xor(and(e, f), and(not(e), g))), add(0x748f82ee, mload(add(wordPointer, 256)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, a), shl(30, a)), or(shr(13, a), shl(19, a))), or(shr(22, a), shl(10, a))), xor(xor(and(a, b), and(a, c)), and(b, c))), 0xffffffff)
                    d := and(add(d, first), 0xffffffff)
                    h := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 320))
                    let y := mload(add(wordPointer, 224))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 288), and(add(add(mload(add(wordPointer, 288)), s0), add(mload(add(wordPointer, 64)), s1)), 0xffffffff))
                    let first := and(add(add(add(g, xor(xor(or(shr(6, d), shl(26, d)), or(shr(11, d), shl(21, d))), or(shr(25, d), shl(7, d)))), xor(and(d, e), and(not(d), f))), add(0x78a5636f, mload(add(wordPointer, 288)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, h), shl(30, h)), or(shr(13, h), shl(19, h))), or(shr(22, h), shl(10, h))), xor(xor(and(h, a), and(h, b)), and(a, b))), 0xffffffff)
                    c := and(add(c, first), 0xffffffff)
                    g := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 352))
                    let y := mload(add(wordPointer, 256))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 320), and(add(add(mload(add(wordPointer, 320)), s0), add(mload(add(wordPointer, 96)), s1)), 0xffffffff))
                    let first := and(add(add(add(f, xor(xor(or(shr(6, c), shl(26, c)), or(shr(11, c), shl(21, c))), or(shr(25, c), shl(7, c)))), xor(and(c, d), and(not(c), e))), add(0x84c87814, mload(add(wordPointer, 320)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, g), shl(30, g)), or(shr(13, g), shl(19, g))), or(shr(22, g), shl(10, g))), xor(xor(and(g, h), and(g, a)), and(h, a))), 0xffffffff)
                    b := and(add(b, first), 0xffffffff)
                    f := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 384))
                    let y := mload(add(wordPointer, 288))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 352), and(add(add(mload(add(wordPointer, 352)), s0), add(mload(add(wordPointer, 128)), s1)), 0xffffffff))
                    let first := and(add(add(add(e, xor(xor(or(shr(6, b), shl(26, b)), or(shr(11, b), shl(21, b))), or(shr(25, b), shl(7, b)))), xor(and(b, c), and(not(b), d))), add(0x8cc70208, mload(add(wordPointer, 352)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, f), shl(30, f)), or(shr(13, f), shl(19, f))), or(shr(22, f), shl(10, f))), xor(xor(and(f, g), and(f, h)), and(g, h))), 0xffffffff)
                    a := and(add(a, first), 0xffffffff)
                    e := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 416))
                    let y := mload(add(wordPointer, 320))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 384), and(add(add(mload(add(wordPointer, 384)), s0), add(mload(add(wordPointer, 160)), s1)), 0xffffffff))
                    let first := and(add(add(add(d, xor(xor(or(shr(6, a), shl(26, a)), or(shr(11, a), shl(21, a))), or(shr(25, a), shl(7, a)))), xor(and(a, b), and(not(a), c))), add(0x90befffa, mload(add(wordPointer, 384)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, e), shl(30, e)), or(shr(13, e), shl(19, e))), or(shr(22, e), shl(10, e))), xor(xor(and(e, f), and(e, g)), and(f, g))), 0xffffffff)
                    h := and(add(h, first), 0xffffffff)
                    d := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 448))
                    let y := mload(add(wordPointer, 352))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 416), and(add(add(mload(add(wordPointer, 416)), s0), add(mload(add(wordPointer, 192)), s1)), 0xffffffff))
                    let first := and(add(add(add(c, xor(xor(or(shr(6, h), shl(26, h)), or(shr(11, h), shl(21, h))), or(shr(25, h), shl(7, h)))), xor(and(h, a), and(not(h), b))), add(0xa4506ceb, mload(add(wordPointer, 416)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, d), shl(30, d)), or(shr(13, d), shl(19, d))), or(shr(22, d), shl(10, d))), xor(xor(and(d, e), and(d, f)), and(e, f))), 0xffffffff)
                    g := and(add(g, first), 0xffffffff)
                    c := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 480))
                    let y := mload(add(wordPointer, 384))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 448), and(add(add(mload(add(wordPointer, 448)), s0), add(mload(add(wordPointer, 224)), s1)), 0xffffffff))
                    let first := and(add(add(add(b, xor(xor(or(shr(6, g), shl(26, g)), or(shr(11, g), shl(21, g))), or(shr(25, g), shl(7, g)))), xor(and(g, h), and(not(g), a))), add(0xbef9a3f7, mload(add(wordPointer, 448)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, c), shl(30, c)), or(shr(13, c), shl(19, c))), or(shr(22, c), shl(10, c))), xor(xor(and(c, d), and(c, e)), and(d, e))), 0xffffffff)
                    f := and(add(f, first), 0xffffffff)
                    b := and(add(first, second), 0xffffffff)
                }
                {
                    let x := mload(add(wordPointer, 0))
                    let y := mload(add(wordPointer, 416))
                    let s0 := xor(xor(or(shr(7, x), shl(25, x)), or(shr(18, x), shl(14, x))), shr(3, x))
                    let s1 := xor(xor(or(shr(17, y), shl(15, y)), or(shr(19, y), shl(13, y))), shr(10, y))
                    mstore(add(wordPointer, 480), and(add(add(mload(add(wordPointer, 480)), s0), add(mload(add(wordPointer, 256)), s1)), 0xffffffff))
                    let first := and(add(add(add(a, xor(xor(or(shr(6, f), shl(26, f)), or(shr(11, f), shl(21, f))), or(shr(25, f), shl(7, f)))), xor(and(f, g), and(not(f), h))), add(0xc67178f2, mload(add(wordPointer, 480)))), 0xffffffff)
                    let second := and(add(xor(xor(or(shr(2, b), shl(30, b)), or(shr(13, b), shl(19, b))), or(shr(22, b), shl(10, b))), xor(xor(and(b, c), and(b, d)), and(c, d))), 0xffffffff)
                    e := and(add(e, first), 0xffffffff)
                    a := and(add(first, second), 0xffffffff)
                }
                mstore(add(statePointer, 0), and(add(mload(add(statePointer, 0)), a), 0xffffffff))
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
                transform(state, words, add(add(input, 32), mul(i, 64)))
            }
            for { let i := 0 } lt(i, 8) { i := add(i, 1) } {
                result := or(result, shl(sub(224, mul(i, 32)), mload(add(state, mul(i, 32)))))
            }
        }
    }
}
