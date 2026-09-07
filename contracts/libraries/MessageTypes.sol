// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {Constants} from "./Constants.sol";

library MessageTypes {
    enum Kind {
        DepositFinality,
        WithdrawalClaim,
        EmergencyExit,
        CheckpointCertificate,
        GovernanceMigration
    }

    struct Envelope {
        Kind kind;
        uint256 sourceChainId;
        uint256 destinationChainId;
        address sourceSender;
        address destinationTarget;
        address contractsManager;
        uint64 nonce;
        uint256 value;
        uint64 gasLimit;
        bytes32 payloadHash;
    }

    error InvalidMessageEnvelope();

    function validate(Envelope memory envelope) internal pure {
        if (
            envelope.sourceChainId == 0 || envelope.destinationChainId == 0
                || envelope.sourceChainId == envelope.destinationChainId || envelope.sourceSender == address(0)
                || envelope.destinationTarget == address(0) || envelope.contractsManager == address(0)
                || envelope.gasLimit < 25_000 || envelope.payloadHash == bytes32(0)
        ) {
            revert InvalidMessageEnvelope();
        }
    }

    function hash(Envelope memory envelope) internal pure returns (bytes32) {
        validate(envelope);
        return keccak256(
            abi.encode(
                Constants.DOMAIN_MESSAGE,
                Constants.PROTOCOL_VERSION,
                envelope.kind,
                envelope.sourceChainId,
                envelope.destinationChainId,
                envelope.sourceSender,
                envelope.destinationTarget,
                envelope.contractsManager,
                envelope.nonce,
                envelope.value,
                envelope.gasLimit,
                envelope.payloadHash
            )
        );
    }
}
