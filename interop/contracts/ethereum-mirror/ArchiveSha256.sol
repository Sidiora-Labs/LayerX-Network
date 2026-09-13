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

    function compress(bytes32 packed, bytes memory input, uint256 blocks) private pure returns (bytes32) {
        uint32[8] memory state;
        for (uint256 i = 0; i < 8; ++i) state[i] = uint32(uint256(packed) >> (224 - 32 * i));
        uint32[64] memory words;
        uint32[64] memory constants = [
            uint32(0x428a2f98), uint32(0x71374491), uint32(0xb5c0fbcf), uint32(0xe9b5dba5),
            uint32(0x3956c25b), uint32(0x59f111f1), uint32(0x923f82a4), uint32(0xab1c5ed5),
            uint32(0xd807aa98), uint32(0x12835b01), uint32(0x243185be), uint32(0x550c7dc3),
            uint32(0x72be5d74), uint32(0x80deb1fe), uint32(0x9bdc06a7), uint32(0xc19bf174),
            uint32(0xe49b69c1), uint32(0xefbe4786), uint32(0x0fc19dc6), uint32(0x240ca1cc),
            uint32(0x2de92c6f), uint32(0x4a7484aa), uint32(0x5cb0a9dc), uint32(0x76f988da),
            uint32(0x983e5152), uint32(0xa831c66d), uint32(0xb00327c8), uint32(0xbf597fc7),
            uint32(0xc6e00bf3), uint32(0xd5a79147), uint32(0x06ca6351), uint32(0x14292967),
            uint32(0x27b70a85), uint32(0x2e1b2138), uint32(0x4d2c6dfc), uint32(0x53380d13),
            uint32(0x650a7354), uint32(0x766a0abb), uint32(0x81c2c92e), uint32(0x92722c85),
            uint32(0xa2bfe8a1), uint32(0xa81a664b), uint32(0xc24b8b70), uint32(0xc76c51a3),
            uint32(0xd192e819), uint32(0xd6990624), uint32(0xf40e3585), uint32(0x106aa070),
            uint32(0x19a4c116), uint32(0x1e376c08), uint32(0x2748774c), uint32(0x34b0bcb5),
            uint32(0x391c0cb3), uint32(0x4ed8aa4a), uint32(0x5b9cca4f), uint32(0x682e6ff3),
            uint32(0x748f82ee), uint32(0x78a5636f), uint32(0x84c87814), uint32(0x8cc70208),
            uint32(0x90befffa), uint32(0xa4506ceb), uint32(0xbef9a3f7), uint32(0xc67178f2)
        ];
        for (uint256 blockIndex = 0; blockIndex < blocks; ++blockIndex) {
            transform(state, words, constants, input, blockIndex * 64);
        }
        uint256 result;
        for (uint256 i = 0; i < 8; ++i) result |= uint256(state[i]) << (224 - 32 * i);
        return bytes32(result);
    }

    function rotate(uint32 value, uint256 bits) private pure returns (uint32) {
        return (value >> bits) | (value << (32 - bits));
    }

    function transform(
        uint32[8] memory state,
        uint32[64] memory words,
        uint32[64] memory constants,
        bytes memory input,
        uint256 offset
    ) private pure {
        for (uint256 i = 0; i < 16; ++i) {
            uint32 word;
            assembly ("memory-safe") {
                word := shr(224, mload(add(add(input, 32), add(offset, mul(i, 4)))))
            }
            words[i] = word;
        }
        unchecked {
            for (uint256 i = 16; i < 64; ++i) {
                uint32 s0 = rotate(words[i - 15], 7) ^ rotate(words[i - 15], 18) ^ (words[i - 15] >> 3);
                uint32 s1 = rotate(words[i - 2], 17) ^ rotate(words[i - 2], 19) ^ (words[i - 2] >> 10);
                words[i] = words[i - 16] + s0 + words[i - 7] + s1;
            }
            uint32 a = state[0];
            uint32 b = state[1];
            uint32 c = state[2];
            uint32 d = state[3];
            uint32 e = state[4];
            uint32 f = state[5];
            uint32 g = state[6];
            uint32 h = state[7];
            for (uint256 i = 0; i < 64; ++i) {
                uint32 s1 = rotate(e, 6) ^ rotate(e, 11) ^ rotate(e, 25);
                uint32 choice = (e & f) ^ ((~e) & g);
                uint32 first = h + s1 + choice + constants[i] + words[i];
                uint32 s0 = rotate(a, 2) ^ rotate(a, 13) ^ rotate(a, 22);
                uint32 majority = (a & b) ^ (a & c) ^ (b & c);
                uint32 second = s0 + majority;
                h = g;
                g = f;
                f = e;
                e = d + first;
                d = c;
                c = b;
                b = a;
                a = first + second;
            }
            state[0] += a;
            state[1] += b;
            state[2] += c;
            state[3] += d;
            state[4] += e;
            state[5] += f;
            state[6] += g;
            state[7] += h;
        }
    }
}
