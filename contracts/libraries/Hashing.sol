// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {Arithmetic} from "./Arithmetic.sol";
import {Constants} from "./Constants.sol";
import {Encoding} from "./Encoding.sol";
import {Types} from "./Types.sol";

library Hashing {
    error HashInputTooLarge(uint256 length);

    function keccakDomain(bytes32 domain, bytes memory value) internal pure returns (bytes32) {
        if (value.length > Constants.MAX_CANONICAL_BYTES) {
            revert HashInputTooLarge(value.length);
        }
        return keccak256(abi.encodePacked(Constants.PROTOCOL_VERSION, domain, Arithmetic.toUint32(value.length), value));
    }

    function sha256Domain(bytes32 domain, bytes memory value) internal pure returns (bytes32) {
        if (value.length > Constants.MAX_CANONICAL_BYTES) {
            revert HashInputTooLarge(value.length);
        }
        return sha256(abi.encodePacked(Constants.PROTOCOL_VERSION, domain, Arithmetic.toUint32(value.length), value));
    }

    function chainBound(bytes32 domain, uint256 chainId, address verifyingContract, bytes32 payloadHash)
        internal
        pure
        returns (bytes32)
    {
        return keccak256(Encoding.domainEnvelope(domain, chainId, verifyingContract, payloadHash));
    }

    function callCommitment(Types.CallCommitment memory operation) internal pure returns (bytes32) {
        return Encoding.callCommitment(operation);
    }
}
