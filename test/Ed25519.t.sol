// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {Ed25519Signer} from "./Ed25519Signer.sol";
import {Ed25519} from "../contracts/crypto/Ed25519.sol";

interface Ed25519Vm {
    function readFile(string calldata path) external view returns (string memory);
    function parseJsonBytes(string calldata json, string calldata key) external pure returns (bytes memory);
    function parseJsonBytes32(string calldata json, string calldata key) external pure returns (bytes32);
    function toString(uint256 value) external pure returns (string memory);
}

contract Ed25519Test {
    Ed25519Vm private constant vm = Ed25519Vm(address(uint160(uint256(keccak256("hevm cheat code")))));
    uint256 private constant L = 2 ** 252 + 27742317777372353535851937790883648493;

    function vector(uint256 index) private view returns (bytes32 key, bytes memory message, bytes memory signature) {
        string memory json = vm.readFile("contracts/config/ed25519-vectors.json");
        string memory base = string.concat(".vectors[", vm.toString(index), "]");
        key = vm.parseJsonBytes32(json, string.concat(base, ".public_key"));
        message = vm.parseJsonBytes(json, string.concat(base, ".message"));
        signature = vm.parseJsonBytes(json, string.concat(base, ".signature"));
    }

    function testSigningHelperReproducesRfc8032VectorOne() external view {
        (bytes32 expectedKey, bytes memory message, bytes memory expectedSignature) = vector(0);
        (bytes32 key, bytes memory signature) =
            Ed25519Signer.sign(hex"9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60", message);
        require(key == expectedKey, "RFC8032 signing public key");
        require(keccak256(signature) == keccak256(expectedSignature), "RFC8032 signing signature");
    }

    function testRfc8032AllFivePureEd25519Vectors() external view {
        for (uint256 i; i < 5; ++i) {
            (bytes32 key, bytes memory message, bytes memory signature) = vector(i);
            require(Ed25519.verify(key, message, signature), "RFC8032 valid signature");
            if (i == 3) require(message.length == 1023, "RFC8032 long message length");
            require(!Ed25519.verify(key, bytes.concat(message, hex"00"), signature), "changed message");
            require(!Ed25519.verify(key ^ bytes32(uint256(1)), message, signature), "changed key");
        }
    }

    function testSha512BlockAndPaddingBoundaries() external view {
        string memory json = vm.readFile("contracts/config/ed25519-vectors.json");
        for (uint256 i; i < 9; ++i) {
            string memory base = string.concat(".sha512[", vm.toString(i), "]");
            bytes memory message = vm.parseJsonBytes(json, string.concat(base, ".message"));
            bytes memory expected = vm.parseJsonBytes(json, string.concat(base, ".digest"));
            require(keccak256(Ed25519.sha512(message)) == keccak256(expected), "SHA512 digest");
        }
        (, bytes memory digest,) = vector(4);
        require(keccak256(Ed25519.sha512(bytes("abc"))) == keccak256(digest), "RFC8032 SHA abc");
    }

    function testEverySignatureByteMutationRefused() external view {
        (bytes32 key, bytes memory message, bytes memory signature) = vector(0);
        for (uint256 i; i < 64; ++i) {
            signature[i] ^= 0x01;
            require(!Ed25519.verify(key, message, signature), "signature byte mutation");
            signature[i] ^= 0x01;
        }
    }

    function testNoncanonicalScalarsAndLengthsRefused() external view {
        (bytes32 key, bytes memory message, bytes memory signature) = vector(0);
        require(!Ed25519.verify(key, message, new bytes(0)), "empty signature");
        require(!Ed25519.verify(key, message, new bytes(63)), "short signature");
        require(!Ed25519.verify(key, message, bytes.concat(signature, hex"00")), "long signature");
        for (uint256 i; i < 32; ++i) {
            signature[32 + i] = bytes1(uint8(L >> (8 * i)));
        }
        require(!Ed25519.verify(key, message, signature), "S equals L");
        signature[32] = bytes1(uint8(signature[32]) + 1);
        require(!Ed25519.verify(key, message, signature), "S exceeds L");
        for (uint256 i = 32; i < 64; ++i) {
            signature[i] = 0xff;
        }
        require(!Ed25519.verify(key, message, signature), "maximal scalar");
    }

    function testMalformedNoncanonicalAndSmallOrderPointsRefused() external view {
        (bytes32 key, bytes memory message, bytes memory signature) = vector(0);
        bytes32[7] memory invalid = [
            bytes32(hex"edffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f"),
            bytes32(hex"ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f"),
            bytes32(hex"0100000000000000000000000000000000000000000000000000000000000080"),
            bytes32(hex"0200000000000000000000000000000000000000000000000000000000000000"),
            bytes32(0),
            bytes32(hex"0100000000000000000000000000000000000000000000000000000000000000"),
            bytes32(hex"ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f")
        ];
        for (uint256 i; i < invalid.length; ++i) {
            require(!Ed25519.verify(invalid[i], message, signature), "invalid authority point");
            bytes memory altered = bytes.concat(invalid[i], new bytes(32));
            for (uint256 j = 32; j < 64; ++j) {
                altered[j] = signature[j];
            }
            require(!Ed25519.verify(key, message, altered), "invalid R point");
        }
        bytes32 identity = invalid[5];
        require(!Ed25519.verify(identity, message, bytes.concat(identity, new bytes(32))), "identity forgery");
    }
}
