// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

library Ed25519 {
    uint256 private constant P = 2 ** 255 - 19;
    uint256 private constant L = 2 ** 252 + 27742317777372353535851937790883648493;
    uint256 private constant D = 37095705934669439343138083508754565189542113879843219016388785533085940283555;
    uint256 private constant SQRT_M1 = 19681161376707505956807079304988542015446066515923890162744021073123829784752;

    struct Point {
        uint256 x;
        uint256 y;
        uint256 z;
        uint256 t;
    }

    function verify(bytes32 publicKey, bytes memory message, bytes memory signature) internal pure returns (bool) {
        if (signature.length != 64) return false;
        bytes32 encodedR;
        assembly ("memory-safe") { encodedR := mload(add(signature, 32)) }
        uint256 s;
        for (uint256 i; i < 32; ++i) {
            s |= uint256(uint8(signature[32 + i])) << (8 * i);
        }
        if (s >= L) return false;
        (bool validA, Point memory a) = decode(publicKey);
        (bool validR, Point memory r) = decode(encodedR);
        if (!validA || !validR || smallOrder(a) || smallOrder(r)) return false;
        bytes memory digest = sha512(abi.encodePacked(encodedR, publicKey, message));
        uint256 h;
        for (uint256 i = 64; i > 0; --i) {
            h = addmod(mulmod(h, 256, L), uint8(digest[i - 1]), L);
        }
        Point memory b = Point(
            15112221349535400772501151409588531511454012693041857206046113283949847762202,
            46316835694926478169428394003475163141307993866256225615783033603165251855960,
            1,
            0
        );
        b.t = mulmod(b.x, b.y, P);
        return equal(multiply(b, s), add(r, multiply(a, h)));
    }

    function power(uint256 a, uint256 exponent) private pure returns (uint256 r) {
        r = 1;
        while (exponent != 0) {
            if (exponent & 1 != 0) r = mulmod(r, a, P);
            a = mulmod(a, a, P);
            exponent >>= 1;
        }
    }

    function decode(bytes32 encoded) private pure returns (bool, Point memory q) {
        uint256 n;
        for (uint256 i; i < 32; ++i) {
            n |= uint256(uint8(encoded[i])) << (8 * i);
        }
        uint256 sign = n >> 255;
        uint256 y = n & (2 ** 255 - 1);
        if (y >= P) return (false, q);
        uint256 yy = mulmod(y, y, P);
        uint256 u = addmod(yy, P - 1, P);
        uint256 v = addmod(mulmod(D, yy, P), 1, P);
        uint256 xx = mulmod(u, power(v, P - 2), P);
        uint256 x = power(xx, (P + 3) / 8);
        if (mulmod(x, x, P) != xx) x = mulmod(x, SQRT_M1, P);
        if (mulmod(x, x, P) != xx || (x == 0 && sign != 0)) return (false, q);
        if ((x & 1) != sign) x = P - x;
        q = Point(x, y, 1, mulmod(x, y, P));
        return (true, q);
    }

    function add(Point memory p, Point memory q) private pure returns (Point memory r) {
        uint256 a = mulmod(addmod(p.y, P - p.x, P), addmod(q.y, P - q.x, P), P);
        uint256 b = mulmod(addmod(p.y, p.x, P), addmod(q.y, q.x, P), P);
        uint256 c = mulmod(mulmod(p.t, q.t, P), 2 * D, P);
        uint256 d = mulmod(2, mulmod(p.z, q.z, P), P);
        uint256 e = addmod(b, P - a, P);
        uint256 f = addmod(d, P - c, P);
        uint256 g = addmod(d, c, P);
        uint256 h = addmod(b, a, P);
        r = Point(mulmod(e, f, P), mulmod(g, h, P), mulmod(f, g, P), mulmod(e, h, P));
    }

    function multiply(Point memory q, uint256 scalar) internal pure returns (Point memory r) {
        r = Point(0, 1, 1, 0);
        while (scalar != 0) {
            if (scalar & 1 != 0) r = add(r, q);
            q = add(q, q);
            scalar >>= 1;
        }
    }

    function equal(Point memory a, Point memory b) private pure returns (bool) {
        return mulmod(a.x, b.z, P) == mulmod(b.x, a.z, P) && mulmod(a.y, b.z, P) == mulmod(b.y, a.z, P);
    }

    function smallOrder(Point memory q) private pure returns (bool) {
        q = add(q, q);
        q = add(q, q);
        q = add(q, q);
        return q.x == 0 && q.y == q.z;
    }

    function rotate(uint64 x, uint256 n) private pure returns (uint64) {
        return (x >> n) | (x << (64 - n));
    }

    function sha512(bytes memory input) internal pure returns (bytes memory output) {
        uint64[80] memory k = [
            uint64(0x428a2f98d728ae22),
            0x7137449123ef65cd,
            0xb5c0fbcfec4d3b2f,
            0xe9b5dba58189dbbc,
            0x3956c25bf348b538,
            0x59f111f1b605d019,
            0x923f82a4af194f9b,
            0xab1c5ed5da6d8118,
            0xd807aa98a3030242,
            0x12835b0145706fbe,
            0x243185be4ee4b28c,
            0x550c7dc3d5ffb4e2,
            0x72be5d74f27b896f,
            0x80deb1fe3b1696b1,
            0x9bdc06a725c71235,
            0xc19bf174cf692694,
            0xe49b69c19ef14ad2,
            0xefbe4786384f25e3,
            0xfc19dc68b8cd5b5,
            0x240ca1cc77ac9c65,
            0x2de92c6f592b0275,
            0x4a7484aa6ea6e483,
            0x5cb0a9dcbd41fbd4,
            0x76f988da831153b5,
            0x983e5152ee66dfab,
            0xa831c66d2db43210,
            0xb00327c898fb213f,
            0xbf597fc7beef0ee4,
            0xc6e00bf33da88fc2,
            0xd5a79147930aa725,
            0x6ca6351e003826f,
            0x142929670a0e6e70,
            0x27b70a8546d22ffc,
            0x2e1b21385c26c926,
            0x4d2c6dfc5ac42aed,
            0x53380d139d95b3df,
            0x650a73548baf63de,
            0x766a0abb3c77b2a8,
            0x81c2c92e47edaee6,
            0x92722c851482353b,
            0xa2bfe8a14cf10364,
            0xa81a664bbc423001,
            0xc24b8b70d0f89791,
            0xc76c51a30654be30,
            0xd192e819d6ef5218,
            0xd69906245565a910,
            0xf40e35855771202a,
            0x106aa07032bbd1b8,
            0x19a4c116b8d2d0c8,
            0x1e376c085141ab53,
            0x2748774cdf8eeb99,
            0x34b0bcb5e19b48a8,
            0x391c0cb3c5c95a63,
            0x4ed8aa4ae3418acb,
            0x5b9cca4f7763e373,
            0x682e6ff3d6b2b8a3,
            0x748f82ee5defb2fc,
            0x78a5636f43172f60,
            0x84c87814a1f0ab72,
            0x8cc702081a6439ec,
            0x90befffa23631e28,
            0xa4506cebde82bde9,
            0xbef9a3f7b2c67915,
            0xc67178f2e372532b,
            0xca273eceea26619c,
            0xd186b8c721c0c207,
            0xeada7dd6cde0eb1e,
            0xf57d4f7fee6ed178,
            0x6f067aa72176fba,
            0xa637dc5a2c898a6,
            0x113f9804bef90dae,
            0x1b710b35131c471b,
            0x28db77f523047d84,
            0x32caab7b40c72493,
            0x3c9ebe0a15c9bebc,
            0x431d67c49c100d4c,
            0x4cc5d4becb3e42b6,
            0x597f299cfc657e2a,
            0x5fcb6fab3ad6faec,
            0x6c44198c4a475817
        ];
        uint64[8] memory state = [
            uint64(0x6a09e667f3bcc908),
            0xbb67ae8584caa73b,
            0x3c6ef372fe94f82b,
            0xa54ff53a5f1d36f1,
            0x510e527fade682d1,
            0x9b05688c2b3e6c1f,
            0x1f83d9abfb41bd6b,
            0x5be0cd19137e2179
        ];
        bytes memory padded = new bytes(((input.length + 17 + 127) / 128) * 128);
        for (uint256 i; i < input.length; ++i) {
            padded[i] = input[i];
        }
        padded[input.length] = 0x80;
        uint256 bits = input.length * 8;
        for (uint256 i; i < 16; ++i) {
            padded[padded.length - 1 - i] = bytes1(uint8(bits >> (8 * i)));
        }
        unchecked {
            for (uint256 offset; offset < padded.length; offset += 128) {
                uint64[80] memory w;
                for (uint256 i; i < 16; ++i) {
                    for (uint256 j; j < 8; ++j) {
                        w[i] = (w[i] << 8) | uint64(uint8(padded[offset + i * 8 + j]));
                    }
                }
                for (uint256 i = 16; i < 80; ++i) {
                    uint64 s0 = rotate(w[i - 15], 1) ^ rotate(w[i - 15], 8) ^ (w[i - 15] >> 7);
                    uint64 s1 = rotate(w[i - 2], 19) ^ rotate(w[i - 2], 61) ^ (w[i - 2] >> 6);
                    w[i] = w[i - 16] + s0 + w[i - 7] + s1;
                }
                uint64[8] memory v;
                for (uint256 i; i < 8; ++i) {
                    v[i] = state[i];
                }
                for (uint256 i; i < 80; ++i) {
                    uint64 s1 = rotate(v[4], 14) ^ rotate(v[4], 18) ^ rotate(v[4], 41);
                    uint64 t1 = v[7] + s1 + ((v[4] & v[5]) ^ (~v[4] & v[6])) + k[i] + w[i];
                    uint64 s0 = rotate(v[0], 28) ^ rotate(v[0], 34) ^ rotate(v[0], 39);
                    uint64 t2 = s0 + ((v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]));
                    v[7] = v[6];
                    v[6] = v[5];
                    v[5] = v[4];
                    v[4] = v[3] + t1;
                    v[3] = v[2];
                    v[2] = v[1];
                    v[1] = v[0];
                    v[0] = t1 + t2;
                }
                for (uint256 i; i < 8; ++i) {
                    state[i] += v[i];
                }
            }
        }
        output = new bytes(64);
        for (uint256 i; i < 8; ++i) {
            for (uint256 j; j < 8; ++j) {
                output[i * 8 + j] = bytes1(uint8(state[i] >> (56 - 8 * j)));
            }
        }
    }
}

contract Ed25519Verifier {
    function verify(bytes32 publicKey, bytes calldata message, bytes calldata signature) external pure returns (bool) {
        return Ed25519.verify(publicKey, message, signature);
    }
}
