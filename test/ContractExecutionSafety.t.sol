// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {Burn} from "../contracts/libraries/Burn.sol";
import {CallerChecker} from "../contracts/libraries/CallerChecker.sol";
import {EOA} from "../contracts/libraries/EOA.sol";
import {SafeCall} from "../contracts/libraries/SafeCall.sol";
import {Storage} from "../contracts/libraries/Storage.sol";
import {TransientContext} from "../contracts/libraries/TransientContext.sol";
import {DelegateCallAware} from "../contracts/security/DelegateCallAware.sol";
import {UUPSNotUpgradeable} from "../contracts/security/UUPSNotUpgradeable.sol";

interface ExecutionVm {
    function deal(address account, uint256 balance) external;
    function etch(address account, bytes calldata code) external;
    function expectPartialRevert(bytes4 selector) external;
    function prank(address sender, address origin) external;
}

contract CallHarness {
    receive() external payable {}

    function perform(address target, bytes calldata input, uint32 maximumCopy)
        external
        returns (bool success, uint256 size, bytes memory output)
    {
        SafeCall.CallResult memory result = SafeCall.call(target, 0, input, 200_000, maximumCopy, true);
        return (result.success, result.returnDataSize, result.returnData);
    }

    function performChecked(address target, bytes calldata input, uint32 maximumCopy) external returns (bytes memory) {
        SafeCall.CallResult memory result = SafeCall.call(target, 0, input, 200_000, maximumCopy, true);
        return SafeCall.requireSuccess(target, result);
    }

    function performStatic(address target, bytes calldata input, uint32 maximumCopy)
        external
        view
        returns (bool, uint256, bytes memory)
    {
        SafeCall.CallResult memory result = SafeCall.staticCall(target, input, 200_000, maximumCopy);
        return (result.success, result.returnDataSize, result.returnData);
    }

    function burnNative(uint256 amount) external {
        Burn.native(amount);
    }
}

contract ReturnBomb {
    fallback() external {
        assembly ("memory-safe") { return(0, 65536) }
    }
}

contract RevertBomb {
    fallback() external {
        assembly ("memory-safe") { revert(0, 65536) }
    }
}

contract StatefulTarget {
    uint256 public value;

    function setValue(uint256 next) external returns (uint256) {
        value = next;
        return next;
    }
}

contract CallerHarness {
    function requireDirectOrigin() external view {
        CallerChecker.requireDirectCodelessOrigin();
    }

    function requireContract() external view {
        CallerChecker.requireContractCaller();
    }

    function delegated(address account) external view returns (bool, address) {
        return (EOA.isDelegatedAccount(account), EOA.delegationTarget(account));
    }
}

contract ConstructorCaller {
    constructor(CallerHarness harness) {
        harness.requireDirectOrigin();
    }
}

contract ExecutionHarness is DelegateCallAware, UUPSNotUpgradeable, TransientContext {
    constructor(bool useTransient) TransientContext(useTransient, keccak256("execution-harness")) {}

    function directOnly() external view onlyDirectCall returns (address) {
        return address(this);
    }

    function delegatedOnly() external view onlyDelegateCall returns (address) {
        return address(this);
    }

    function useContext(bytes32 value, bool nested) external scopedContext(value) returns (bytes32) {
        if (nested) this.useContext(keccak256("nested"), false);
        return executionContext();
    }
}

contract DelegateProxy {
    address private immutable implementation;

    constructor(address target) {
        implementation = target;
    }

    fallback() external payable {
        address target = implementation;
        assembly ("memory-safe") {
            calldatacopy(0, 0, calldatasize())
            let success := delegatecall(gas(), target, 0, calldatasize(), 0, 0)
            returndatacopy(0, 0, returndatasize())
            switch success
            case 0 { revert(0, returndatasize()) }
            default { return(0, returndatasize()) }
        }
    }
}

contract StorageHarness {
    bytes32 private immutable valueSlot = Storage.derive(keccak256("storage-harness"), keccak256("value"));

    function set(bytes32 value) external {
        Storage.storeBytes32(valueSlot, value);
    }

    function get() external view returns (bytes32) {
        return Storage.loadBytes32(valueSlot);
    }

    function derived(bytes32 component, bytes32 field) external pure returns (bytes32) {
        return Storage.derive(component, field);
    }

    function validate(bytes32 slot) external pure {
        Storage.validate(slot);
    }
}

contract ContractExecutionSafetyTest {
    ExecutionVm private constant vm = ExecutionVm(address(uint160(uint256(keccak256("hevm cheat code")))));

    function testSafeCallBoundsSuccessfulReturnData() public {
        CallHarness harness = new CallHarness();
        ReturnBomb target = new ReturnBomb();
        (bool success, uint256 size, bytes memory output) = harness.perform(address(target), "", 64);
        require(success, "call failed");
        require(size == 65_536 && output.length == 64, "return not bounded");
    }

    function testSafeCallBoundsFailureWithoutLosingClassification() public {
        CallHarness harness = new CallHarness();
        RevertBomb target = new RevertBomb();
        (bool success, uint256 size, bytes memory output) = harness.perform(address(target), "", 96);
        require(!success, "revert classified success");
        require(size == 65_536 && output.length == 96, "revert not bounded");
        vm.expectPartialRevert(SafeCall.CallFailed.selector);
        harness.performChecked(address(target), "", 96);
    }

    function testSafeCallRejectsEmptyCodeAndStaticMutation() public {
        CallHarness harness = new CallHarness();
        vm.expectPartialRevert(SafeCall.TargetHasNoCode.selector);
        harness.perform(address(0xBEEF), "", 32);
        StatefulTarget target = new StatefulTarget();
        (bool success,,) = harness.performStatic(address(target), abi.encodeCall(StatefulTarget.setValue, (7)), 32);
        require(!success && target.value() == 0, "static mutation");
    }

    function testBurnUsesCanonicalSinkWithoutSelfDestruct() public {
        CallHarness harness = new CallHarness();
        vm.deal(address(harness), 2 ether);
        address sink = 0x000000000000000000000000000000000000dEaD;
        uint256 beforeBalance = sink.balance;
        harness.burnNative(1 ether);
        require(address(harness).balance == 1 ether, "source balance");
        require(sink.balance == beforeBalance + 1 ether, "sink balance");
    }

    function testDirectCodelessOriginRuleRejectsContractsAndConstructors() public {
        CallerHarness harness = new CallerHarness();
        address direct = address(0xA11CE);
        vm.prank(direct, direct);
        harness.requireDirectOrigin();
        vm.expectPartialRevert(CallerChecker.DirectCodelessOriginOnly.selector);
        harness.requireDirectOrigin();
        harness.requireContract();
        vm.expectPartialRevert(CallerChecker.DirectCodelessOriginOnly.selector);
        new ConstructorCaller(harness);
    }

    function testEIP7702DelegatedOriginIsNotCodeless() public {
        CallerHarness harness = new CallerHarness();
        address delegated = address(0x7702);
        address delegate = address(0xD311);
        vm.etch(delegated, abi.encodePacked(hex"ef0100", delegate));
        (bool detected, address target) = harness.delegated(delegated);
        require(detected && target == delegate, "delegation designator");
        vm.expectPartialRevert(CallerChecker.DirectCodelessOriginOnly.selector);
        vm.prank(delegated, delegated);
        harness.requireDirectOrigin();
    }

    function testDirectAndDelegateOnlyPostures() public {
        ExecutionHarness implementation = new ExecutionHarness(false);
        DelegateProxy proxy = new DelegateProxy(address(implementation));
        require(implementation.directOnly() == address(implementation), "direct");
        vm.expectPartialRevert(DelegateCallAware.DirectCallOnly.selector);
        ExecutionHarness(address(proxy)).directOnly();
        vm.expectPartialRevert(DelegateCallAware.DelegateCallOnly.selector);
        implementation.delegatedOnly();
        require(ExecutionHarness(address(proxy)).delegatedOnly() == address(proxy), "delegate");
    }

    function testUUPSSelectorsFailClosedDirectAndDelegated() public {
        ExecutionHarness implementation = new ExecutionHarness(false);
        DelegateProxy proxy = new DelegateProxy(address(implementation));
        vm.expectPartialRevert(UUPSNotUpgradeable.UpgradesPermanentlyDisabled.selector);
        implementation.upgradeTo(address(0x1234));
        vm.expectPartialRevert(UUPSNotUpgradeable.UpgradesPermanentlyDisabled.selector);
        ExecutionHarness(address(proxy)).upgradeToAndCall(address(0x1234), "");
    }

    function testPersistentAndTransientContextClearAfterScope() public {
        ExecutionHarness persistent = new ExecutionHarness(false);
        bytes32 context = keccak256("persistent");
        require(persistent.useContext(context, false) == context, "persistent context");
        require(persistent.executionContext() == bytes32(0), "persistent uncleared");
        ExecutionHarness transientHarness = new ExecutionHarness(true);
        context = keccak256("transient");
        require(transientHarness.useContext(context, false) == context, "transient context");
        require(transientHarness.executionContext() == bytes32(0), "transient uncleared");
    }

    function testNestedContextAndReservedStorageSlotsReject() public {
        ExecutionHarness context = new ExecutionHarness(false);
        vm.expectPartialRevert(TransientContext.ContextAlreadyActive.selector);
        context.useContext(keccak256("outer"), true);
        StorageHarness storageHarness = new StorageHarness();
        vm.expectPartialRevert(Storage.ReservedStorageSlot.selector);
        storageHarness.validate(Storage.EIP1967_IMPLEMENTATION_SLOT);
        vm.expectPartialRevert(Storage.InvalidStorageNamespace.selector);
        storageHarness.derived(bytes32(0), keccak256("field"));
    }

    function testNamespacedStorageRoundTrip() public {
        StorageHarness harness = new StorageHarness();
        bytes32 value = keccak256("stored");
        harness.set(value);
        require(harness.get() == value, "storage round trip");
    }
}
