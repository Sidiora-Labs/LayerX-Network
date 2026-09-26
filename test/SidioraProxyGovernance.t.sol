// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {SidioraProxyTimelock, ISidioraProxyUpgrade} from "../contracts/governance/SidioraProxyTimelock.sol";
import {SidioraProxyGovernance} from "../scripts/SidioraProxyGovernance.s.sol";
import {ERC1967Proxy} from "../contracts/lib/openzeppelin-contracts/contracts/proxy/ERC1967/ERC1967Proxy.sol";
import {ERC1967Utils} from "../contracts/lib/openzeppelin-contracts/contracts/proxy/ERC1967/ERC1967Utils.sol";
import {UUPSUpgradeable} from "../contracts/lib/openzeppelin-contracts/contracts/proxy/utils/UUPSUpgradeable.sol";
import {Initializable} from "../contracts/lib/openzeppelin-contracts/contracts/proxy/utils/Initializable.sol";
import {Ownable} from "../contracts/lib/openzeppelin-contracts/contracts/access/Ownable.sol";

interface SidioraProxyVm {
    function warp(uint256 timestamp) external;
    function getBlockTimestamp() external view returns (uint256);
    function prank(address sender) external;
    function expectRevert(bytes4 selector) external;
    function expectRevert(bytes calldata reason) external;
    function expectPartialRevert(bytes4 selector) external;
    function expectEmit(bool topic1, bool topic2, bool topic3, bool data, address emitter) external;
    function load(address target, bytes32 slot) external view returns (bytes32);
    function chainId(uint256 chain) external;
    function parseJsonString(string calldata json, string calldata key) external pure returns (string memory);
    function toString(address value) external pure returns (string memory);
}

interface ISidioraInitialize {
    function initialize() external;
}

contract GovernedProxyImplementation is UUPSUpgradeable, Ownable, Initializable {
    constructor() Ownable(msg.sender) {
        _disableInitializers();
    }

    function initialize(address initialOwner) external initializer {
        if (initialOwner == address(0)) revert OwnableInvalidOwner(initialOwner);
        _transferOwnership(initialOwner);
    }

    function _authorizeUpgrade(address) internal override onlyOwner {}
}

contract GovernedSidioraReplacement is GovernedProxyImplementation {
    bool public sidioraInitialized;

    function initialize() external reinitializer(2) {
        sidioraInitialized = true;
    }
}

contract SidioraProxyGovernanceTest {
    SidioraProxyVm private constant vm = SidioraProxyVm(address(uint160(uint256(keccak256("hevm cheat code")))));
    address private constant FOUNDATION = address(0xF0);
    address private constant GOVERNANCE = address(0xA1);
    address private constant EXECUTOR = address(0xB2);
    address private constant GUARDIAN = address(0xC3);
    address private constant OUTSIDER = address(0xD4);
    uint64 private constant DELAY = 2 days;
    uint64 private constant FLOOR = 1 days;
    uint64 private constant GRACE = 7 days;
    bytes32 private constant SALT = keccak256("sidiora-proxy-upgrade");

    SidioraProxyGovernance private runner;
    SidioraProxyTimelock private timelock;
    GovernedProxyImplementation private original;
    GovernedSidioraReplacement private replacement;
    address private proxy;
    bytes private data;

    event OperationScheduled(
        bytes32 indexed operationId, address indexed target, uint256 value, bytes32 dataHash, uint64 readyAt
    );
    event OperationCancelled(bytes32 indexed operationId, address indexed target, uint256 value, bytes32 dataHash);
    event OperationExecuted(bytes32 indexed operationId, address indexed target, uint256 value, bytes32 dataHash);

    function setUp() public {
        original = new GovernedProxyImplementation();
        replacement = new GovernedSidioraReplacement();
        proxy = address(new ERC1967Proxy(address(original), abi.encodeCall(original.initialize, (FOUNDATION))));
        runner = new SidioraProxyGovernance();
        timelock = runner.deploy(FOUNDATION, _parameters());
        data = abi.encodeCall(ISidioraProxyUpgrade.upgradeToAndCall, (address(replacement), bytes("")));
        bytes32 preflight = _schedule(data);
        vm.prank(GUARDIAN);
        timelock.cancel(preflight);
        runner.handover(FOUNDATION, timelock, _parameters());
    }

    function testScriptHandsOwnershipToGovernanceTimelock() public view {
        require(GovernedProxyImplementation(proxy).owner() == address(timelock), "handover absent");
        require(timelock.proxy() == proxy, "proxy changed");
        require(timelock.proposer(GOVERNANCE) && !timelock.proposer(FOUNDATION), "proposer mismatch");
        require(timelock.executor() == EXECUTOR && timelock.guardian() == GUARDIAN, "role mismatch");
        require(timelock.minDelay() == DELAY && timelock.delayFloor() == FLOOR, "delay mismatch");
        require(timelock.gracePeriod() == GRACE, "grace mismatch");
        require(timelock.callPermission(proxy, ISidioraProxyUpgrade.upgradeToAndCall.selector), "permission absent");
    }

    function testScriptSchedulesAndUpgradeExecutesAtReadyTime() public {
        bytes memory initialized = _initializedUpgrade();
        bytes32 expected = timelock.operationId(proxy, 0, initialized, SALT, 1);
        vm.expectEmit(true, true, false, true, address(timelock));
        emit OperationScheduled(expected, proxy, 0, sha256(initialized), uint64(block.timestamp + DELAY));
        (bytes32 id, uint256 nonce) = runner.scheduleUpgrade(timelock, _parameters(), address(replacement), SALT, DELAY);
        require(id == expected && nonce == 1, "script operation mismatch");
        require(timelock.operationNonce() == 2, "nonce absent");
        vm.warp(timelock.readyAt(id) - 1);
        vm.expectRevert(SidioraProxyTimelock.OperationNotReady.selector);
        _execute(initialized, SALT, nonce);
        require(_implementation() == address(original), "early upgrade");
        vm.warp(timelock.readyAt(id));
        vm.expectEmit(true, true, false, true, address(timelock));
        emit OperationExecuted(id, proxy, 0, sha256(initialized));
        _execute(initialized, SALT, nonce);
        require(_implementation() == address(replacement), "upgrade absent");
        require(GovernedProxyImplementation(proxy).owner() == address(timelock), "owner storage changed");
        require(timelock.completed(id), "completion absent");
    }

    function testScheduledUpgradeDataIsExactlyUpgradeToAndCallInitialize() public {
        bytes memory expected = _initializedUpgrade();
        bytes memory encoded = runner.upgradeCall(address(replacement));
        require(encoded.length == expected.length && keccak256(encoded) == keccak256(expected), "upgrade call differs");
        require(bytes4(encoded) == ISidioraProxyUpgrade.upgradeToAndCall.selector, "upgrade selector differs");
        (address implementation, bytes memory initialization) = abi.decode(_slice(encoded, 4), (address, bytes));
        require(implementation == address(replacement), "implementation differs");
        require(
            initialization.length == 4 && bytes4(initialization) == ISidioraInitialize.initialize.selector
                && bytes4(initialization) == bytes4(keccak256("initialize()")),
            "initialize call differs"
        );
        bytes32 expectedId = timelock.operationId(proxy, 0, expected, SALT, 1);
        vm.expectEmit(true, true, false, true, address(timelock));
        emit OperationScheduled(expectedId, proxy, 0, sha256(expected), uint64(block.timestamp + DELAY));
        (bytes32 id, uint256 nonce) = runner.scheduleUpgrade(timelock, _parameters(), address(replacement), SALT, DELAY);
        require(id == expectedId && nonce == 1, "scheduled data differs");
        require(id != timelock.operationId(proxy, 0, data, SALT, nonce), "scheduled data omits initialize");
    }

    function testScheduleUpgradeAcceptsNoCallerSuppliedBytes() public {
        require(
            SidioraProxyGovernance.scheduleUpgrade.selector
                == bytes4(
                    keccak256(
                        "scheduleUpgrade(address,(address,address,address,address,uint64,uint64,uint64),address,bytes32,uint64)"
                    )
                ),
            "schedule signature carries caller bytes"
        );
        bytes4 withBytes = bytes4(
            keccak256(
                "scheduleUpgrade(address,(address,address,address,address,uint64,uint64,uint64),address,bytes,bytes32,uint64)"
            )
        );
        (bool accepted,) = address(runner)
            .call(
                abi.encodeWithSelector(
                    withBytes,
                    timelock,
                    _parameters(),
                    address(replacement),
                    abi.encodeCall(original.initialize, (OUTSIDER)),
                    SALT,
                    DELAY
                )
            );
        require(!accepted, "caller bytes accepted");
        require(timelock.operationNonce() == 1, "caller bytes reached the schedule");
        (bool extended,) = address(runner)
            .call(
                bytes.concat(
                    abi.encodeCall(
                        runner.scheduleUpgrade, (timelock, _parameters(), address(replacement), SALT, DELAY)
                    ),
                    abi.encode(abi.encodeCall(original.initialize, (OUTSIDER)))
                )
            );
        require(extended, "trailing bytes changed the schedule call");
        bytes32 id = timelock.operationId(proxy, 0, _initializedUpgrade(), SALT, 1);
        require(timelock.readyAt(id) == block.timestamp + DELAY, "trailing bytes reached the scheduled data");
    }

    function testExecutedUpgradeInitializesReplacementOnce() public {
        require(!replacement.sidioraInitialized(), "implementation initialized outside the proxy");
        (bytes32 id, uint256 nonce) = runner.scheduleUpgrade(timelock, _parameters(), address(replacement), SALT, DELAY);
        vm.warp(timelock.readyAt(id));
        _execute(_initializedUpgrade(), SALT, nonce);
        require(_implementation() == address(replacement), "upgrade absent");
        require(GovernedSidioraReplacement(proxy).sidioraInitialized(), "replacement not initialized");
        require(GovernedProxyImplementation(proxy).owner() == address(timelock), "initialization changed owner");
        vm.expectRevert(Initializable.InvalidInitialization.selector);
        GovernedSidioraReplacement(proxy).initialize();
        vm.expectRevert(Initializable.InvalidInitialization.selector);
        vm.prank(address(timelock));
        GovernedSidioraReplacement(proxy).initialize();
        require(GovernedSidioraReplacement(proxy).sidioraInitialized(), "second initialize changed state");
    }

    function testGraceBoundaryIsInclusive() public {
        bytes32 id = _schedule(data);
        vm.warp(uint256(timelock.readyAt(id)) + GRACE);
        _execute(data, SALT, 1);
        require(_implementation() == address(replacement), "last eligible execution refused");
    }

    function testExpiredOperationCannotExecute() public {
        bytes32 id = _schedule(data);
        vm.warp(uint256(timelock.readyAt(id)) + GRACE + 1);
        vm.expectRevert(SidioraProxyTimelock.OperationNotReady.selector);
        _execute(data, SALT, 1);
        require(_implementation() == address(original) && !timelock.completed(id), "expired operation executed");
    }

    function testGuardianCancelsBeforeReadyWithOperationDetails() public {
        bytes32 id = _schedule(data);
        uint64 ready = timelock.readyAt(id);
        vm.warp(ready - 1);
        vm.expectEmit(true, true, false, true, address(timelock));
        emit OperationCancelled(id, proxy, 0, sha256(data));
        vm.prank(GUARDIAN);
        timelock.cancel(id);
        require(timelock.readyAt(id) == 0, "cancellation absent");
        vm.warp(ready);
        vm.expectRevert(SidioraProxyTimelock.OperationNotReady.selector);
        _execute(data, SALT, 1);
    }

    function testGuardianCannotCancelReadyOperation() public {
        bytes32 id = _schedule(data);
        vm.warp(timelock.readyAt(id));
        vm.expectRevert(SidioraProxyTimelock.OperationNotReady.selector);
        vm.prank(GUARDIAN);
        timelock.cancel(id);
        _execute(data, SALT, 1);
    }

    function testGuardianCannotCancelUnknownOrAlreadyCancelledOperation() public {
        vm.expectRevert(SidioraProxyTimelock.OperationNotReady.selector);
        vm.prank(GUARDIAN);
        timelock.cancel(bytes32(0));
        bytes32 id = _schedule(data);
        vm.prank(GUARDIAN);
        timelock.cancel(id);
        vm.expectRevert(SidioraProxyTimelock.OperationNotReady.selector);
        vm.prank(GUARDIAN);
        timelock.cancel(id);
    }

    function testOnlyGovernanceSchedulesOnlyExecutorExecutesOnlyGuardianCancels() public {
        address[4] memory accounts = [FOUNDATION, EXECUTOR, GUARDIAN, OUTSIDER];
        for (uint256 i; i < accounts.length; ++i) {
            vm.expectRevert(SidioraProxyTimelock.Unauthorized.selector);
            vm.prank(accounts[i]);
            timelock.schedule(proxy, 0, data, SALT, DELAY);
        }
        bytes32 id = _schedule(data);
        accounts = [FOUNDATION, GOVERNANCE, EXECUTOR, OUTSIDER];
        for (uint256 i; i < accounts.length; ++i) {
            vm.expectRevert(SidioraProxyTimelock.Unauthorized.selector);
            vm.prank(accounts[i]);
            timelock.cancel(id);
        }
        vm.warp(timelock.readyAt(id));
        accounts = [FOUNDATION, GOVERNANCE, GUARDIAN, OUTSIDER];
        for (uint256 i; i < accounts.length; ++i) {
            vm.expectRevert(SidioraProxyTimelock.Unauthorized.selector);
            vm.prank(accounts[i]);
            timelock.execute(proxy, 0, data, SALT, 1);
        }
    }

    function testFormerOwnerCannotUpgradeDirectly() public {
        vm.expectRevert(abi.encodeWithSelector(Ownable.OwnableUnauthorizedAccount.selector, FOUNDATION));
        vm.prank(FOUNDATION);
        GovernedProxyImplementation(proxy).upgradeToAndCall(address(replacement), "");
        require(_implementation() == address(original), "foundation retained upgrade authority");
    }

    function testUnpermittedSelectorsTargetsAndValueAreRefused() public {
        vm.expectRevert(SidioraProxyTimelock.InvalidOperation.selector);
        _schedule(abi.encodeCall(Ownable.transferOwnership, (FOUNDATION)));
        vm.expectRevert(SidioraProxyTimelock.InvalidOperation.selector);
        _schedule(abi.encodeCall(Ownable.renounceOwnership, ()));
        vm.expectRevert(SidioraProxyTimelock.InvalidOperation.selector);
        vm.prank(GOVERNANCE);
        timelock.schedule(address(original), 0, data, SALT, DELAY);
        vm.expectRevert(SidioraProxyTimelock.InvalidOperation.selector);
        vm.prank(GOVERNANCE);
        timelock.schedule(proxy, 1, data, SALT, DELAY);
        vm.expectRevert(SidioraProxyTimelock.InvalidOperation.selector);
        vm.prank(GOVERNANCE);
        timelock.schedule(
            address(timelock),
            0,
            abi.encodeWithSignature(
                "setCallPermission(address,bytes4,bool)", proxy, Ownable.transferOwnership.selector, true
            ),
            SALT,
            DELAY
        );
    }

    function testDelayBelowMinimumIsRefused() public {
        vm.expectRevert(SidioraProxyTimelock.InvalidOperation.selector);
        vm.prank(GOVERNANCE);
        timelock.schedule(proxy, 0, data, SALT, DELAY - 1);
    }

    function testLongerRequestedDelayIsRespected() public {
        vm.prank(GOVERNANCE);
        bytes32 id = timelock.schedule(proxy, 0, data, SALT, DELAY + FLOOR);
        require(timelock.readyAt(id) == block.timestamp + DELAY + FLOOR, "requested delay lost");
        vm.warp(block.timestamp + DELAY);
        vm.expectRevert(SidioraProxyTimelock.OperationNotReady.selector);
        _execute(data, SALT, 1);
    }

    function testCompletedOperationCannotReplayOrBeCancelled() public {
        bytes32 id = _schedule(data);
        vm.warp(timelock.readyAt(id));
        _execute(data, SALT, 1);
        vm.expectRevert(SidioraProxyTimelock.OperationNotReady.selector);
        _execute(data, SALT, 1);
        vm.expectRevert(SidioraProxyTimelock.OperationNotReady.selector);
        vm.prank(GUARDIAN);
        timelock.cancel(id);
    }

    function testCancelledOperationRescheduleHasNewIdentityAndDelay() public {
        bytes32 first = _schedule(data);
        vm.prank(GUARDIAN);
        timelock.cancel(first);
        vm.warp(vm.getBlockTimestamp() + FLOOR);
        bytes32 second = _schedule(data);
        require(
            first != second && timelock.readyAt(second) == vm.getBlockTimestamp() + DELAY, "reschedule reused operation"
        );
        vm.warp(timelock.readyAt(second));
        vm.expectRevert(SidioraProxyTimelock.OperationNotReady.selector);
        _execute(data, SALT, 1);
        _execute(data, SALT, 2);
    }

    function testOperationBindsChainTimelockTargetValueDataSaltAndNonce() public {
        bytes32 id = _schedule(data);
        vm.warp(timelock.readyAt(id));
        bytes memory changed = abi.encodeCall(ISidioraProxyUpgrade.upgradeToAndCall, (address(original), bytes("")));
        vm.expectRevert(SidioraProxyTimelock.OperationNotReady.selector);
        _execute(changed, SALT, 1);
        vm.expectRevert(SidioraProxyTimelock.OperationNotReady.selector);
        _execute(data, bytes32(0), 1);
        vm.expectRevert(SidioraProxyTimelock.OperationNotReady.selector);
        _execute(data, SALT, 2);
        vm.expectRevert(SidioraProxyTimelock.OperationNotReady.selector);
        vm.prank(EXECUTOR);
        timelock.execute(address(original), 0, data, SALT, 1);
        vm.expectRevert(SidioraProxyTimelock.OperationNotReady.selector);
        vm.prank(EXECUTOR);
        timelock.execute(proxy, 1, data, SALT, 1);
        SidioraProxyTimelock other = runner.deploy(FOUNDATION, _parameters());
        require(id != other.operationId(proxy, 0, data, SALT, 1), "timelock not bound");
        vm.chainId(block.chainid + 1);
        vm.expectRevert(SidioraProxyTimelock.OperationNotReady.selector);
        _execute(data, SALT, 1);
    }

    function testRevertedUpgradeDoesNotCompleteOrChangeImplementation() public {
        bytes memory failing = abi.encodeCall(
            ISidioraProxyUpgrade.upgradeToAndCall,
            (address(replacement), abi.encodeCall(original.initialize, (FOUNDATION)))
        );
        bytes32 id = _schedule(failing);
        vm.warp(timelock.readyAt(id));
        vm.expectPartialRevert(SidioraProxyTimelock.CallFailed.selector);
        _execute(failing, SALT, 1);
        require(!timelock.completed(id), "failed operation completed");
        require(_implementation() == address(original), "failed initialization changed implementation");
        require(GovernedProxyImplementation(proxy).owner() == address(timelock), "failed initialization changed owner");
    }

    function testInvalidImplementationAndNoncanonicalDataAreRefused() public {
        vm.expectRevert(SidioraProxyTimelock.InvalidOperation.selector);
        _schedule(abi.encodeCall(ISidioraProxyUpgrade.upgradeToAndCall, (OUTSIDER, bytes(""))));
        vm.expectRevert(SidioraProxyTimelock.InvalidOperation.selector);
        _schedule(abi.encodeCall(ISidioraProxyUpgrade.upgradeToAndCall, (proxy, bytes(""))));
        vm.expectRevert(SidioraProxyTimelock.InvalidOperation.selector);
        _schedule(abi.encodeCall(ISidioraProxyUpgrade.upgradeToAndCall, (address(timelock), bytes(""))));
        vm.expectRevert(SidioraProxyTimelock.InvalidOperation.selector);
        _schedule(bytes.concat(data, hex"00"));
        vm.expectRevert(SidioraProxyTimelock.InvalidOperation.selector);
        _schedule(hex"123456");
    }

    function testConstructorRejectsZeroRolesMissingProxyAndInvalidDelays() public {
        SidioraProxyGovernance.Parameters memory p = _parameters();
        p.minimumDelay = FLOOR - 1;
        _expectInvalidDeployment(p);
        p = _parameters();
        p.delayFloor = 0;
        _expectInvalidDeployment(p);
        p = _parameters();
        p.gracePeriod = 0;
        _expectInvalidDeployment(p);
        p = _parameters();
        p.proxy = OUTSIDER;
        _expectInvalidDeployment(p);
        p = _parameters();
        p.governanceAuthority = address(0);
        _expectInvalidDeployment(p);
        p = _parameters();
        p.executor = address(0);
        _expectInvalidDeployment(p);
        p = _parameters();
        p.guardian = address(0);
        _expectInvalidDeployment(p);
    }

    function testHandoverRequiresScheduleOnTheConfiguredTimelock() public {
        SidioraProxyGovernance.Parameters memory p = _parameters();
        p.proxy = address(new ERC1967Proxy(address(original), abi.encodeCall(original.initialize, (FOUNDATION))));
        SidioraProxyTimelock unconfirmed = runner.deploy(FOUNDATION, p);
        vm.expectRevert(SidioraProxyGovernance.GovernanceScheduleUnconfirmed.selector);
        runner.handover(FOUNDATION, unconfirmed, p);
        require(GovernedProxyImplementation(p.proxy).owner() == FOUNDATION, "unconfirmed handover changed owner");
        vm.expectRevert(SidioraProxyTimelock.Unauthorized.selector);
        vm.prank(OUTSIDER);
        unconfirmed.schedule(p.proxy, 0, data, SALT, DELAY);
        vm.expectRevert(SidioraProxyGovernance.GovernanceScheduleUnconfirmed.selector);
        runner.handover(FOUNDATION, unconfirmed, p);
        require(unconfirmed.operationNonce() == 0, "unauthorized schedule confirmed governance");
        require(GovernedProxyImplementation(p.proxy).owner() == FOUNDATION, "failed preflight changed owner");
        vm.prank(GOVERNANCE);
        bytes32 id = unconfirmed.schedule(p.proxy, 0, data, SALT, DELAY);
        runner.handover(FOUNDATION, unconfirmed, p);
        require(GovernedProxyImplementation(p.proxy).owner() == address(unconfirmed), "confirmed handover absent");
        vm.warp(unconfirmed.readyAt(id));
        vm.prank(EXECUTOR);
        unconfirmed.execute(p.proxy, 0, data, SALT, 0);
        require(
            address(uint160(uint256(vm.load(p.proxy, ERC1967Utils.IMPLEMENTATION_SLOT)))) == address(replacement),
            "confirmed governance upgrade absent"
        );
    }

    function testHandoverDoesNotTreatAuthorityCodeAsConfirmation() public {
        SidioraProxyGovernance.Parameters memory p = _parameters();
        p.proxy = address(new ERC1967Proxy(address(original), abi.encodeCall(original.initialize, (FOUNDATION))));
        p.governanceAuthority = address(original);
        SidioraProxyTimelock unconfirmed = runner.deploy(FOUNDATION, p);
        vm.expectRevert(SidioraProxyGovernance.GovernanceScheduleUnconfirmed.selector);
        runner.handover(FOUNDATION, unconfirmed, p);
        require(GovernedProxyImplementation(p.proxy).owner() == FOUNDATION, "authority code permitted handover");
    }

    function testScriptRejectsMismatchedTimelockConfiguration() public {
        SidioraProxyGovernance.Parameters memory p = _parameters();
        p.minimumDelay++;
        vm.expectRevert(SidioraProxyGovernance.InvalidConfiguration.selector);
        runner.handover(FOUNDATION, timelock, p);
        vm.expectRevert(SidioraProxyGovernance.InvalidConfiguration.selector);
        runner.scheduleUpgrade(timelock, p, address(replacement), SALT, DELAY);
        vm.expectRevert(SidioraProxyGovernance.InvalidConfiguration.selector);
        runner.proposalBody(timelock, p);
    }

    function testScriptRejectsWrongOwnerRepeatedHandoverAndPrematureSchedule() public {
        vm.expectRevert(SidioraProxyGovernance.InvalidConfiguration.selector);
        runner.handover(OUTSIDER, timelock, _parameters());
        vm.expectRevert(SidioraProxyGovernance.InvalidConfiguration.selector);
        runner.handover(FOUNDATION, timelock, _parameters());
        vm.expectRevert(SidioraProxyGovernance.InvalidConfiguration.selector);
        runner.handover(address(timelock), timelock, _parameters());
        SidioraProxyTimelock other = runner.deploy(FOUNDATION, _parameters());
        vm.expectRevert(SidioraProxyGovernance.InvalidConfiguration.selector);
        runner.scheduleUpgrade(other, _parameters(), address(replacement), SALT, DELAY);
    }

    function testProposalBodyNamesActualProxyTimelockRolesAndDelay() public view {
        string memory body = runner.proposalBody(timelock, _parameters());
        require(
            keccak256(bytes(vm.parseJsonString(body, ".content['@type']")))
                == keccak256("/cosmos.gov.v1beta1.TextProposal"),
            "proposal type mismatch"
        );
        string memory description = vm.parseJsonString(body, ".content.description");
        require(_contains(description, vm.toString(proxy)), "proxy absent");
        require(_contains(description, vm.toString(address(timelock))), "timelock absent");
        require(_contains(description, vm.toString(GOVERNANCE)), "governance absent");
        require(_contains(description, vm.toString(EXECUTOR)), "executor absent");
        require(_contains(description, vm.toString(GUARDIAN)), "guardian absent");
        require(_contains(description, "minimum delay 172800 seconds"), "delay absent");
        require(_contains(description, "delay floor 86400 seconds"), "floor absent");
        require(_contains(description, "execution grace period 604800 seconds"), "grace absent");
        require(
            _contains(description, "passage does not execute an EVM call"), "text proposal execution misrepresented"
        );
    }

    function _parameters() private view returns (SidioraProxyGovernance.Parameters memory) {
        return SidioraProxyGovernance.Parameters(proxy, GOVERNANCE, EXECUTOR, GUARDIAN, DELAY, FLOOR, GRACE);
    }

    function _schedule(bytes memory callData) private returns (bytes32) {
        vm.prank(GOVERNANCE);
        return timelock.schedule(proxy, 0, callData, SALT, DELAY);
    }

    function _execute(bytes memory callData, bytes32 salt, uint256 nonce) private {
        vm.prank(EXECUTOR);
        timelock.execute(proxy, 0, callData, salt, nonce);
    }

    function _initializedUpgrade() private view returns (bytes memory) {
        return abi.encodeCall(
            ISidioraProxyUpgrade.upgradeToAndCall,
            (address(replacement), abi.encodeCall(ISidioraInitialize.initialize, ()))
        );
    }

    function _slice(bytes memory value, uint256 start) private pure returns (bytes memory result) {
        result = new bytes(value.length - start);
        for (uint256 i; i < result.length; ++i) {
            result[i] = value[start + i];
        }
    }

    function _implementation() private view returns (address) {
        return address(uint160(uint256(vm.load(proxy, ERC1967Utils.IMPLEMENTATION_SLOT))));
    }

    function _expectInvalidDeployment(SidioraProxyGovernance.Parameters memory p) private {
        vm.expectRevert(SidioraProxyTimelock.InvalidOperation.selector);
        new SidioraProxyTimelock(
            p.proxy, p.governanceAuthority, p.executor, p.guardian, p.minimumDelay, p.delayFloor, p.gracePeriod
        );
    }

    function _contains(string memory value, string memory needle) private pure returns (bool) {
        bytes memory haystack = bytes(value);
        bytes memory search = bytes(needle);
        if (search.length > haystack.length) return false;
        for (uint256 i; i <= haystack.length - search.length; ++i) {
            uint256 j;
            while (j < search.length && haystack[i + j] == search[j]) ++j;
            if (j == search.length) return true;
        }
        return false;
    }
}
