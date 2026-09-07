// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {Governed} from "../security/Governed.sol";
import {LayerXComponent} from "../security/LayerXComponent.sol";
import {Predeploys} from "../deployment/Predeploys.sol";

contract WithdrawalNullifierRegistry is Governed, LayerXComponent {
    enum Status {
        None,
        Reserved,
        Consumed,
        Cancelled
    }

    error ConsumerOnly();
    error NullifierAlreadyUsed();
    error InvalidTransition();

    mapping(address => bool) public consumer;
    mapping(bytes32 => Status) public status;
    mapping(bytes32 => bytes32) public claimForNullifier;
    mapping(bytes32 => bool) public withdrawalIdUsed;
    mapping(bytes32 => bytes32) public withdrawalIdForNullifier;

    event ConsumerSet(address indexed consumer, bool enabled);
    event NullifierTransition(
        bytes32 indexed nullifier, bytes32 indexed claimId, Status previousStatus, Status newStatus
    );

    constructor(
        address governanceTimelock,
        address emergencyAuthority,
        bytes32 componentConfigHash,
        uint192 componentRelease
    )
        Governed(governanceTimelock, emergencyAuthority)
        LayerXComponent(Predeploys.NULLIFIER_REGISTRY, componentConfigHash, componentRelease)
    {}

    modifier onlyConsumer() {
        if (!consumer[msg.sender]) revert ConsumerOnly();
        _;
    }

    function setConsumer(address account, bool enabled) external onlyGovernance {
        if (account.code.length == 0) revert ConsumerOnly();
        consumer[account] = enabled;
        emit ConsumerSet(account, enabled);
    }

    function reserve(bytes32 nullifier, bytes32 withdrawalId, bytes32 claimId) external onlyConsumer {
        if (
            nullifier == bytes32(0) || withdrawalId == bytes32(0) || claimId == bytes32(0)
                || status[nullifier] != Status.None || withdrawalIdUsed[withdrawalId]
        ) revert NullifierAlreadyUsed();
        status[nullifier] = Status.Reserved;
        claimForNullifier[nullifier] = claimId;
        withdrawalIdUsed[withdrawalId] = true;
        withdrawalIdForNullifier[nullifier] = withdrawalId;
        emit NullifierTransition(nullifier, claimId, Status.None, Status.Reserved);
    }

    function consume(bytes32 nullifier, bytes32 claimId) external onlyConsumer {
        _transition(nullifier, claimId, Status.Consumed);
    }

    function cancel(bytes32 nullifier, bytes32 claimId) external onlyConsumer {
        _transition(nullifier, claimId, Status.Cancelled);
    }

    function _transition(bytes32 nullifier, bytes32 claimId, Status next) private {
        if (status[nullifier] != Status.Reserved || claimForNullifier[nullifier] != claimId) {
            revert InvalidTransition();
        }
        status[nullifier] = next;
        emit NullifierTransition(nullifier, claimId, Status.Reserved, next);
    }
}
