// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {SidioraProxyTimelock, ISidioraProxyUpgrade} from "../contracts/governance/SidioraProxyTimelock.sol";

interface SidioraGovernanceVm {
    function startBroadcast(address sender) external;
    function stopBroadcast() external;
    function toString(address value) external pure returns (string memory);
    function toString(uint256 value) external pure returns (string memory);
}

interface ISidioraNativeInitialize {
    function initialize() external;
}

interface ISidioraProxyOwnership {
    function owner() external view returns (address);
    function transferOwnership(address newOwner) external;
}

contract SidioraProxyGovernance {
    bool public constant IS_SCRIPT = true;
    SidioraGovernanceVm private constant vm =
        SidioraGovernanceVm(address(uint160(uint256(keccak256("hevm cheat code")))));

    error InvalidConfiguration();
    error GovernanceScheduleUnconfirmed();

    struct Parameters {
        address proxy;
        address governanceAuthority;
        address executor;
        address guardian;
        uint64 minimumDelay;
        uint64 delayFloor;
        uint64 gracePeriod;
    }

    function deploy(address deployer, Parameters calldata parameters) external returns (SidioraProxyTimelock timelock) {
        if (deployer == address(0)) revert InvalidConfiguration();
        vm.startBroadcast(deployer);
        timelock = new SidioraProxyTimelock(
            parameters.proxy,
            parameters.governanceAuthority,
            parameters.executor,
            parameters.guardian,
            parameters.minimumDelay,
            parameters.delayFloor,
            parameters.gracePeriod
        );
        vm.stopBroadcast();
    }

    function handover(address currentOwner, SidioraProxyTimelock timelock, Parameters calldata parameters) external {
        _validate(timelock, parameters);
        ISidioraProxyOwnership proxy = ISidioraProxyOwnership(parameters.proxy);
        if (currentOwner == address(0) || currentOwner == address(timelock) || proxy.owner() != currentOwner) {
            revert InvalidConfiguration();
        }
        if (timelock.operationNonce() == 0) revert GovernanceScheduleUnconfirmed();
        vm.startBroadcast(currentOwner);
        proxy.transferOwnership(address(timelock));
        vm.stopBroadcast();
        if (proxy.owner() != address(timelock)) revert InvalidConfiguration();
    }

    function scheduleUpgrade(
        SidioraProxyTimelock timelock,
        Parameters calldata parameters,
        address implementation,
        bytes32 salt,
        uint64 delay
    ) external returns (bytes32 id, uint256 nonce) {
        _validate(timelock, parameters);
        if (ISidioraProxyOwnership(parameters.proxy).owner() != address(timelock)) revert InvalidConfiguration();
        bytes memory data = upgradeCall(implementation);
        nonce = timelock.operationNonce();
        vm.startBroadcast(parameters.governanceAuthority);
        id = timelock.schedule(parameters.proxy, 0, data, salt, delay);
        vm.stopBroadcast();
    }

    function upgradeCall(address implementation) public pure returns (bytes memory) {
        return abi.encodeCall(
            ISidioraProxyUpgrade.upgradeToAndCall,
            (implementation, abi.encodeCall(ISidioraNativeInitialize.initialize, ()))
        );
    }

    function proposalBody(SidioraProxyTimelock timelock, Parameters calldata parameters)
        external
        view
        returns (string memory)
    {
        _validate(timelock, parameters);
        return string.concat(
            '{"content":{"@type":"/cosmos.gov.v1beta1.TextProposal","title":"Sidiora proxy governance handover",',
            '"description":"Paxeer X Network authorises the foundation to call transferOwnership(',
            vm.toString(address(timelock)),
            ") on the existing Sidiora proxy ",
            vm.toString(parameters.proxy),
            ". SidioraProxyTimelock proposer is the chain governance authority ",
            vm.toString(parameters.governanceAuthority),
            ", executor ",
            vm.toString(parameters.executor),
            ", guardian ",
            vm.toString(parameters.guardian),
            ", minimum delay ",
            vm.toString(uint256(parameters.minimumDelay)),
            " seconds, delay floor ",
            vm.toString(uint256(parameters.delayFloor)),
            " seconds and execution grace period ",
            vm.toString(uint256(parameters.gracePeriod)),
            " seconds. Only upgradeToAndCall(address,bytes) is permitted at this proxy, with zero call value. ",
            "The guardian may cancel before readiness. This text proposal authorises the owner handover; ",
            'passage does not execute an EVM call. No token replacement or holder migration is authorised."}}'
        );
    }

    function _validate(SidioraProxyTimelock timelock, Parameters calldata parameters) private view {
        if (
            address(timelock).code.length == 0 || timelock.proxy() != parameters.proxy
                || timelock.governanceAuthority() != parameters.governanceAuthority
                || timelock.executor() != parameters.executor || timelock.guardian() != parameters.guardian
                || timelock.minDelay() != parameters.minimumDelay || timelock.delayFloor() != parameters.delayFloor
                || timelock.gracePeriod() != parameters.gracePeriod
        ) revert InvalidConfiguration();
    }
}
