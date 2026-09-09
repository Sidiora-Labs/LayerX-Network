// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {Ed25519} from "../contracts/crypto/Ed25519.sol";

library Ed25519Signer {
    uint256 private constant P = 2 ** 255 - 19;
    uint256 private constant L = 2 ** 252 + 27742317777372353535851937790883648493;

    function sign(bytes32 seed, bytes memory message)
        internal
        pure
        returns (bytes32 publicKey, bytes memory signature)
    {
        bytes memory expanded = Ed25519.sha512(abi.encodePacked(seed));
        expanded[0] &= 0xf8;
        expanded[31] = (expanded[31] & 0x3f) | 0x40;
        uint256 scalar;
        bytes memory prefix = new bytes(32);
        for (uint256 i; i < 32; ++i) {
            scalar |= uint256(uint8(expanded[i])) << (8 * i);
            prefix[i] = expanded[32 + i];
        }
        publicKey = baseMultiply(scalar);
        uint256 nonce = reduce(Ed25519.sha512(bytes.concat(prefix, message)));
        bytes32 r = baseMultiply(nonce);
        uint256 challenge = reduce(Ed25519.sha512(abi.encodePacked(r, publicKey, message)));
        uint256 s = addmod(nonce, mulmod(challenge, scalar, L), L);
        signature = abi.encodePacked(r, littleEndian(s));
    }

    function baseMultiply(uint256 scalar) private pure returns (bytes32) {
        Ed25519.Point memory base = Ed25519.Point(
            15112221349535400772501151409588531511454012693041857206046113283949847762202,
            46316835694926478169428394003475163141307993866256225615783033603165251855960,
            1,
            0
        );
        base.t = mulmod(base.x, base.y, P);
        Ed25519.Point memory point = Ed25519.multiply(base, scalar);
        uint256 inverse = 1;
        uint256 exponent = P - 2;
        uint256 z = point.z;
        while (exponent != 0) {
            if (exponent & 1 != 0) inverse = mulmod(inverse, z, P);
            z = mulmod(z, z, P);
            exponent >>= 1;
        }
        uint256 x = mulmod(point.x, inverse, P);
        uint256 y = mulmod(point.y, inverse, P);
        return littleEndian(y | ((x & 1) << 255));
    }

    function reduce(bytes memory digest) private pure returns (uint256 value) {
        for (uint256 i = digest.length; i > 0; --i) {
            value = addmod(mulmod(value, 256, L), uint8(digest[i - 1]), L);
        }
    }

    function littleEndian(uint256 value) private pure returns (bytes32 result) {
        for (uint256 i; i < 32; ++i) {
            result |= bytes32(uint256(uint8(value >> (8 * i))) << (248 - 8 * i));
        }
    }
}
