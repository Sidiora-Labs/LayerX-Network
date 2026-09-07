// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {Constants} from "./Constants.sol";
import {SafeCall} from "./SafeCall.sol";

interface IERC1271Minimal {
    function isValidSignature(bytes32 digest, bytes calldata signature) external view returns (bytes4);
}

library CryptographyPrimitives {
    error InvalidSignatureLength(uint256 length);
    error InvalidSignatureV(uint8 v);
    error InvalidSignatureS(bytes32 s);
    error InvalidSignature();

    bytes4 internal constant ERC1271_MAGIC_VALUE = 0x1626ba7e;

    function domainSeparator(uint256 chainId, address verifyingContract, bytes32 deploymentConfigHash)
        internal
        pure
        returns (bytes32)
    {
        return keccak256(
            abi.encode(
                Constants.DOMAIN_SIGNATURE, Constants.PROTOCOL_VERSION, chainId, verifyingContract, deploymentConfigHash
            )
        );
    }

    function signingDigest(bytes32 separator, bytes32 structHash) internal pure returns (bytes32) {
        return keccak256(abi.encodePacked("\x19\x01", separator, structHash));
    }

    function recover(bytes32 digest, bytes calldata signature) internal pure returns (address signer) {
        bytes32 r;
        bytes32 s;
        uint8 v;
        if (signature.length == 65) {
            assembly ("memory-safe") {
                r := calldataload(signature.offset)
                s := calldataload(add(signature.offset, 32))
                v := byte(0, calldataload(add(signature.offset, 64)))
            }
        } else if (signature.length == 64) {
            bytes32 compactS;
            assembly ("memory-safe") {
                r := calldataload(signature.offset)
                compactS := calldataload(add(signature.offset, 32))
            }
            s = compactS & bytes32(0x7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff);
            v = uint8((uint256(compactS) >> 255) + 27);
        } else {
            revert InvalidSignatureLength(signature.length);
        }
        if (v != 27 && v != 28) revert InvalidSignatureV(v);
        if (uint256(s) == 0 || uint256(s) > Constants.SECP256K1_HALF_ORDER) {
            revert InvalidSignatureS(s);
        }
        signer = ecrecover(digest, v, r, s);
        if (signer == address(0)) revert InvalidSignature();
    }

    function recoverOrZero(bytes32 digest, bytes32 r, bytes32 s, uint8 v) internal pure returns (address) {
        if ((v != 27 && v != 28) || uint256(r) == 0 || uint256(s) == 0 || uint256(s) > Constants.SECP256K1_HALF_ORDER) {
            return address(0);
        }
        return ecrecover(digest, v, r, s);
    }

    function isValidSignature(address expectedSigner, bytes32 digest, bytes calldata signature)
        internal
        view
        returns (bool)
    {
        if (expectedSigner == address(0)) return false;
        if (expectedSigner.code.length == 0) {
            return recover(digest, signature) == expectedSigner;
        }
        SafeCall.CallResult memory result = SafeCall.staticCall(
            expectedSigner, abi.encodeCall(IERC1271Minimal.isValidSignature, (digest, signature)), gasleft(), 32
        );
        return
            result.success && result.returnDataSize == 32
                && abi.decode(result.returnData, (bytes4)) == ERC1271_MAGIC_VALUE;
    }
}
