// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {LayerXTimelock, LayerXTimelockCore} from "../contracts/governance/LayerXTimelock.sol";
import {LayerXVault} from "../contracts/custody/LayerXVault.sol";
import {AssetRegistry} from "../contracts/custody/AssetRegistry.sol";

interface DepositAuthorityVm {
    function warp(uint256 timestamp) external;
    function expectRevert(bytes4 selector) external;
}

contract DepositAuthorityTimelockTest {
    DepositAuthorityVm private constant vm =
        DepositAuthorityVm(address(uint160(uint256(keccak256("hevm cheat code")))));

    function testAuthorityRequiresPermissionAndBothGovernanceDelays() public {
        bytes32 config = keccak256("deposit-authority-timelock");
        uint192 release = uint192(1) << 128;
        LayerXTimelock timelock =
            new LayerXTimelock(1 days, 7 days, address(this), address(this), address(this), 0, config, release);
        AssetRegistry registry = new AssetRegistry(address(timelock), address(this), config, release);
        LayerXVault vault = new LayerXVault(registry, address(timelock), address(this), config, release);
        bytes32 key = 0xd75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a;
        bytes memory data = abi.encodeCall(LayerXVault.setDepositRootAuthority, (key));
        bytes32 salt = keccak256("authority");
        require(vault.depositRootAuthority() == bytes32(0), "unexpected authority");
        vm.expectRevert(LayerXTimelockCore.InvalidOperation.selector);
        timelock.schedule(address(vault), 0, data, salt, 1 days);
        bytes memory permission = abi.encodeCall(
            LayerXTimelockCore.setCallPermission, (address(vault), LayerXVault.setDepositRootAuthority.selector, true)
        );
        bytes32 permissionOperation = timelock.schedule(address(timelock), 0, permission, bytes32(0), 1 days);
        vm.expectRevert(LayerXTimelockCore.OperationNotReady.selector);
        timelock.execute(address(timelock), 0, permission, bytes32(0), 0);
        vm.warp(timelock.readyAt(permissionOperation));
        timelock.execute(address(timelock), 0, permission, bytes32(0), 0);
        bytes32 authorityOperation = timelock.schedule(address(vault), 0, data, salt, 1 days);
        vm.expectRevert(LayerXTimelockCore.OperationNotReady.selector);
        timelock.execute(address(vault), 0, data, salt, 1);
        require(vault.depositRootAuthority() == bytes32(0), "authority changed before delay");
        vm.warp(timelock.readyAt(authorityOperation));
        vm.expectRevert(LayerXTimelockCore.OperationNotReady.selector);
        timelock.execute(
            address(vault), 0, abi.encodeCall(LayerXVault.setDepositRootAuthority, (bytes32(uint256(1)))), salt, 1
        );
        timelock.execute(address(vault), 0, data, salt, 1);
        require(vault.depositRootAuthority() == key, "authority mismatch");
        vm.expectRevert(LayerXTimelockCore.OperationNotReady.selector);
        timelock.execute(address(vault), 0, data, salt, 1);
    }
}
