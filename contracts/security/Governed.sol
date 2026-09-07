// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

abstract contract Governed {
    error GovernanceOnly();
    error EmergencyCouncilOnly();
    error InvalidAuthority();

    address public immutable governance;
    address public immutable emergencyCouncil;

    constructor(address governanceTimelock, address emergencyAuthority) {
        if (governanceTimelock == address(0) || emergencyAuthority == address(0)) revert InvalidAuthority();
        governance = governanceTimelock;
        emergencyCouncil = emergencyAuthority;
    }

    modifier onlyGovernance() {
        if (msg.sender != governance) revert GovernanceOnly();
        _;
    }

    modifier onlyEmergencyCouncil() {
        if (msg.sender != emergencyCouncil) revert EmergencyCouncilOnly();
        _;
    }
}
