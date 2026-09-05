// SPDX-License-Identifier: LicenseRef-Centra-ai-Protocol
pragma solidity ^0.8.24;

import {LayerXTimelock, LayerXTimelockCore} from "../contracts/governance/LayerXTimelock.sol";

import {LayerXBetaTimelock} from "../contracts/governance/LayerXBetaTimelock.sol";

interface TimelockVm {
    function chainId(uint256 chain) external;
    function warp(uint256 timestamp) external;
    function expectRevert(bytes4 selector) external;
}

contract LayerXTimelockTest {
    TimelockVm private constant vm = TimelockVm(address(uint160(uint256(keccak256("hevm cheat code")))));
    bytes32 private constant CONFIG = keccak256("timelock-test-config");
    uint192 private constant RELEASE = uint192(1) << 128;

    function testGovernanceCannotBypassDelay() public {
        LayerXTimelock timelock =
            new LayerXTimelock(2 days, 7 days, address(this), address(this), address(this), 1 ether, CONFIG, RELEASE);
        bytes memory data = abi.encodeCall(LayerXTimelockCore.updateMinDelay, (3 days));
        bytes32 salt = keccak256("delay-update");
        timelock.schedule(address(timelock), 0, data, salt, 2 days);
        vm.expectRevert(LayerXTimelockCore.OperationNotReady.selector);
        timelock.execute(address(timelock), 0, data, salt, 0);
        vm.warp(block.timestamp + 2 days);
        timelock.execute(address(timelock), 0, data, salt, 0);
        require(timelock.minDelay() == 3 days, "delay not enacted");
    }

    function testGuardianCancelsQueuedOperation() public {
        LayerXTimelock timelock =
            new LayerXTimelock(2 days, 7 days, address(this), address(this), address(this), 1 ether, CONFIG, RELEASE);
        bytes memory data = abi.encodeCall(LayerXTimelockCore.updateMinDelay, (3 days));
        bytes32 salt = keccak256("cancelled-update");
        bytes32 id = timelock.schedule(address(timelock), 0, data, salt, 2 days);
        timelock.cancel(id);
        vm.warp(block.timestamp + 2 days);
        vm.expectRevert(LayerXTimelockCore.OperationNotReady.selector);
        timelock.execute(address(timelock), 0, data, salt, 0);
    }

    function testRoleChangesRequireTheirOwnCompletedDelay() public {
        LayerXTimelock timelock =
            new LayerXTimelock(2 days, 7 days, address(this), address(this), address(this), 1 ether, CONFIG, RELEASE);
        address newProposer = address(0xA11CE);
        bytes memory data = abi.encodeCall(LayerXTimelockCore.setRole, (uint8(1), newProposer, true));
        bytes32 salt = keccak256("proposer-role");
        timelock.schedule(address(timelock), 0, data, salt, 2 days);
        require(!timelock.proposer(newProposer), "role changed before execution");
        vm.warp(block.timestamp + 2 days);
        timelock.execute(address(timelock), 0, data, salt, 0);
        require(timelock.proposer(newProposer), "delayed role change absent");
    }

    function testImmediateBetaExecutesWithoutAdvancingTimeAndRefusesReplay() public {
        vm.chainId(125);
        LayerXBetaTimelock timelock =
            new LayerXBetaTimelock(0, 7 days, address(this), address(this), address(this), 1 ether, CONFIG, RELEASE);
        uint256 timestamp = block.timestamp;
        address proposer = address(0xA11CE);
        bytes memory data = abi.encodeCall(LayerXTimelockCore.setRole, (uint8(1), proposer, true));
        bytes32 salt = keccak256("immediate-beta-proposer");
        bytes32 operation = timelock.schedule(address(timelock), 0, data, salt, 0);
        require(timelock.readyAt(operation) == timestamp, "beta operation delayed");
        require(!timelock.proposer(proposer), "scheduling changed role");
        timelock.execute(address(timelock), 0, data, salt, 0);
        require(timelock.proposer(proposer), "beta role absent");
        require(block.timestamp == timestamp, "test advanced clock");
        vm.expectRevert(LayerXTimelockCore.OperationNotReady.selector);
        timelock.execute(address(timelock), 0, data, salt, 0);
        vm.expectRevert(LayerXTimelockCore.Unauthorized.selector);
        timelock.setRole(1, address(0xB0B), true);
        vm.expectRevert(LayerXTimelockCore.OperationNotReady.selector);
        timelock.execute(address(timelock), 0, data, keccak256("wrong-salt"), 0);
    }

    function testImmediateBetaRetainsTargetAllowlistAndRoleAuthorization() public {
        vm.chainId(125);
        LayerXBetaTimelock timelock =
            new LayerXBetaTimelock(0, 7 days, address(this), address(this), address(this), 1 ether, CONFIG, RELEASE);
        LayerXTimelock target =
            new LayerXTimelock(1 days, 7 days, address(this), address(this), address(this), 1 ether, CONFIG, RELEASE);
        bytes memory data = abi.encodeCall(LayerXTimelockCore.updateMinDelay, (2 days));
        vm.expectRevert(LayerXTimelockCore.InvalidOperation.selector);
        timelock.schedule(address(target), 0, data, bytes32(0), 0);
        LayerXBetaTimelock restricted =
            new LayerXBetaTimelock(0, 7 days, address(0xA), address(0xB), address(0xC), 1 ether, CONFIG, RELEASE);
        vm.expectRevert(LayerXTimelockCore.Unauthorized.selector);
        restricted.schedule(address(restricted), 0, data, bytes32(0), 0);
        vm.expectRevert(LayerXTimelockCore.Unauthorized.selector);
        restricted.execute(address(restricted), 0, data, bytes32(0), 0);
    }

    function testImmediateProfileCannotChangeStandardConstructorOrOtherChains() public {
        vm.chainId(125);
        vm.expectRevert(LayerXTimelockCore.InvalidOperation.selector);
        new LayerXTimelock(0, 7 days, address(this), address(this), address(this), 1 ether, CONFIG, RELEASE);
        vm.expectRevert(LayerXTimelockCore.InvalidOperation.selector);
        new LayerXBetaTimelock(1, 7 days, address(this), address(this), address(this), 1 ether, CONFIG, RELEASE);
        vm.chainId(1);
        vm.expectRevert(LayerXTimelockCore.InvalidOperation.selector);
        new LayerXBetaTimelock(0, 7 days, address(this), address(this), address(this), 1 ether, CONFIG, RELEASE);
    }
}
