// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {Arithmetic} from "./Arithmetic.sol";
import {Constants} from "./Constants.sol";
import {Types} from "./Types.sol";

library Encoding {
    error CanonicalValueTooLarge(uint256 length, uint256 maximum);

    function lengthPrefix(bytes calldata value) internal pure returns (bytes memory) {
        if (value.length > Constants.MAX_CANONICAL_BYTES) {
            revert CanonicalValueTooLarge(value.length, Constants.MAX_CANONICAL_BYTES);
        }
        return abi.encodePacked(Arithmetic.toUint32(value.length), value);
    }

    function domainEnvelope(bytes32 domain, uint256 chainId, address verifyingContract, bytes32 payloadHash)
        internal
        pure
        returns (bytes memory)
    {
        return abi.encode(Constants.PROTOCOL_VERSION, domain, chainId, verifyingContract, payloadHash);
    }

    function callCommitment(Types.CallCommitment memory operation) internal pure returns (bytes32) {
        return keccak256(
            abi.encode(
                Constants.DOMAIN_MIGRATION,
                Constants.PROTOCOL_VERSION,
                operation.target,
                operation.value,
                operation.dataHash,
                operation.expectedCodeHashBefore,
                operation.expectedCodeHashAfter
            )
        );
    }

    function orderedPair(bytes32 left, bytes32 right) internal pure returns (bytes memory) {
        return abi.encode(left, right);
    }
}
