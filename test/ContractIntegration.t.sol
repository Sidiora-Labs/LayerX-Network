// SPDX-License-Identifier: LicenseRef-Centra-ai-Protocol
pragma solidity ^0.8.24;

import {AssetRegistry} from "../contracts/custody/AssetRegistry.sol";
import {LayerXVault} from "../contracts/custody/LayerXVault.sol";
import {ReserveReconciler} from "../contracts/custody/ReserveReconciler.sol";
import {CheckpointRegistry} from "../contracts/CheckpointRegistry.sol";
import {GuarantorBond} from "../contracts/GuarantorBond.sol";
import {CheckpointChallengeManager} from "../contracts/challenge/CheckpointChallengeManager.sol";
import {WithdrawalNullifierRegistry} from "../contracts/storage/WithdrawalNullifierRegistry.sol";
import {WithdrawalClaims} from "../contracts/WithdrawalClaims.sol";
import {EmergencyExit} from "../contracts/EmergencyExit.sol";
import {LayerXTimelock} from "../contracts/governance/LayerXTimelock.sol";
import {LayerXCustody} from "../contracts/LayerXCustody.sol";
import {Blueprint} from "../contracts/deployment/Blueprint.sol";
import {Predeploys} from "../contracts/deployment/Predeploys.sol";
import {ILayerXComponent, Preinstalls} from "../contracts/deployment/Preinstalls.sol";
import {ManagerContainer} from "../contracts/manager/ManagerContainer.sol";
import {ManagerMigrator} from "../contracts/manager/ManagerMigrator.sol";
import {ManagerUnauthorized} from "../contracts/manager/BlockErrors.sol";
import {StaticConfig} from "../contracts/config/StaticConfig.sol";
import {Features} from "../contracts/config/Features.sol";
import {Constants} from "../contracts/libraries/Constants.sol";
import {CanonicalCheckpoint} from "../contracts/libraries/CanonicalCheckpoint.sol";
import {MessageTypes} from "../contracts/libraries/MessageTypes.sol";
import {PaxeerWithdrawalCodec} from "../contracts/libraries/PaxeerWithdrawalCodec.sol";
import {SemverComp} from "../contracts/libraries/SemverComp.sol";
import {Governed} from "../contracts/security/Governed.sol";
import {UUPSNotUpgradeable} from "../contracts/security/UUPSNotUpgradeable.sol";

interface IntegrationVm {
    function addr(uint256 privateKey) external returns (address);

    function assume(bool condition) external;

    function deal(address account, uint256 balance) external;

    function expectPartialRevert(bytes4 selector) external;

    function prank(address sender) external;

    function sign(uint256 privateKey, bytes32 digest) external returns (uint8 v, bytes32 r, bytes32 s);

    function warp(uint256 timestamp) external;
}

contract IntegrationToken {
    error TokenUnauthorized();
    error TokenInsufficientBalance();
    error TokenInsufficientAllowance();

    address public immutable owner;
    uint8 public constant decimals = 6;
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;

    constructor(address tokenOwner) {
        owner = tokenOwner;
    }

    function mint(address recipient, uint256 amount) external {
        if (msg.sender != owner) revert TokenUnauthorized();
        balanceOf[recipient] += amount;
    }

    function approve(address spender, uint256 amount) external returns (bool) {
        allowance[msg.sender][spender] = amount;
        return true;
    }

    function transfer(address recipient, uint256 amount) external returns (bool) {
        _transfer(msg.sender, recipient, amount);
        return true;
    }

    function transferFrom(address sender, address recipient, uint256 amount) external returns (bool) {
        uint256 permitted = allowance[sender][msg.sender];
        if (permitted < amount) revert TokenInsufficientAllowance();
        allowance[sender][msg.sender] = permitted - amount;
        _transfer(sender, recipient, amount);
        return true;
    }

    function _transfer(address sender, address recipient, uint256 amount) private {
        if (balanceOf[sender] < amount) revert TokenInsufficientBalance();
        balanceOf[sender] -= amount;
        balanceOf[recipient] += amount;
    }
}

contract GovernanceTarget {
    error DeliberateFailure(uint256 code);

    uint256 public value;

    function setValue(uint256 next) external {
        value = next;
    }

    function fail() external pure {
        revert DeliberateFailure(17);
    }
}

contract MessageTypesTest {
    IntegrationVm private constant vm = IntegrationVm(address(uint160(uint256(keccak256("hevm cheat code")))));

    function testMessageHashRejectsCrossChainAndCrossManagerReplay() public pure {
        MessageTypes.Envelope memory message = _message();
        bytes32 original = MessageTypes.hash(message);
        message.destinationChainId = 9_999;
        require(MessageTypes.hash(message) != original, "destination replay");
        message = _message();
        message.sourceChainId = 9_998;
        require(MessageTypes.hash(message) != original, "source replay");
        message = _message();
        message.contractsManager = address(0x9999);
        require(MessageTypes.hash(message) != original, "manager replay");
        message = _message();
        message.destinationTarget = address(0x8888);
        require(MessageTypes.hash(message) != original, "target replay");
    }

    function testFuzz_MessageHashBindsNonceValueGasAndPayload(
        uint64 nonce,
        uint128 value,
        uint64 gasLimit,
        bytes32 payload
    ) public {
        vm.assume(gasLimit >= 25_000);
        vm.assume(payload != bytes32(0));
        MessageTypes.Envelope memory message = _message();
        message.nonce = nonce;
        message.value = value;
        message.gasLimit = gasLimit;
        message.payloadHash = payload;
        bytes32 original = MessageTypes.hash(message);
        message.payloadHash = keccak256(abi.encode(payload));
        vm.assume(message.payloadHash != bytes32(0) && message.payloadHash != payload);
        require(MessageTypes.hash(message) != original, "payload replay");
    }

    function _message() private pure returns (MessageTypes.Envelope memory) {
        return MessageTypes.Envelope({
            kind: MessageTypes.Kind.WithdrawalClaim,
            sourceChainId: 100,
            destinationChainId: 200,
            sourceSender: address(0x1000),
            destinationTarget: address(0x2000),
            contractsManager: address(0x3000),
            nonce: 7,
            value: 11,
            gasLimit: 100_000,
            payloadHash: keccak256("payload")
        });
    }
}

contract ContractIntegrationTest {
    struct EmergencyCheckpointFixture {
        bytes32 account;
        address recipient;
        uint128 balance;
        bytes32 stateRoot;
        bytes32 checkpointHash;
        CanonicalCheckpoint.GuarantorAttestation[] attestations;
    }

    struct ScheduledCall {
        address target;
        bytes data;
        bytes32 salt;
        uint256 nonce;
    }

    IntegrationVm private constant vm = IntegrationVm(address(uint160(uint256(keccak256("hevm cheat code")))));

    bytes32 private constant GENESIS_RECEIPT_ROOT = keccak256("integration-genesis-receipt-root");
    address private constant EMERGENCY_COUNCIL = address(0xEC01);
    address private constant FINAL_PROPOSER = address(0xA110CE);
    address private constant FINAL_EXECUTOR = address(0xE0EC);

    uint192 private release;
    bytes32 private configHash;
    StaticConfig.Config private deploymentConfig;
    IntegrationToken private token;
    Blueprint private blueprint;
    LayerXTimelock private timelock;
    AssetRegistry private assetRegistry;
    LayerXVault private vault;
    GuarantorBond private guarantorBond;
    CheckpointRegistry private checkpointRegistry;
    CheckpointChallengeManager private challengeManager;
    WithdrawalNullifierRegistry private nullifierRegistry;
    WithdrawalClaims private withdrawalClaims;
    EmergencyExit private emergencyExit;
    ReserveReconciler private reserveReconciler;
    ManagerContainer private managerContainer;
    ManagerMigrator private managerMigrator;
    LayerXCustody private custodyTopology;
    uint256 private governanceSaltNonce;
    bool private injectRoleTransitionFailure;
    bool private roleTransitionFailureObserved;

    receive() external payable {}

    function setUp() public {
        release = SemverComp.parseRelease("1.0.0");
        token = new IntegrationToken(address(this));
        StaticConfig.AssetDefinition[] memory assets = new StaticConfig.AssetDefinition[](1);
        assets[0] = StaticConfig.AssetDefinition({
            assetId: keccak256("USDX"),
            token: address(token),
            tokenDecimals: 6,
            protocolDecimals: 18,
            minimumDeposit: 1_000_000,
            custodyCap: 1_000_000_000_000
        });
        deploymentConfig = StaticConfig.Config({
            chainId: block.chainid,
            protocolVersion: Constants.PROTOCOL_VERSION,
            releaseVersion: release,
            governanceTimelock: address(0),
            emergencyCouncil: EMERGENCY_COUNCIL,
            genesisReceiptRoot: GENESIS_RECEIPT_ROOT,
            challengeWindow: 7 days,
            checkpointLivenessBound: 1 days,
            enabledFeatures: Features.ERC20_CUSTODY | Features.CHECKPOINT_CHALLENGES | Features.WITHDRAWAL_CLAIMS
                | Features.EMERGENCY_EXIT | Features.RESERVE_RECONCILIATION,
            assetDefinitionsRoot: StaticConfig.hashAssets(assets)
        });
        StaticConfig.Config memory blueprintConfig = deploymentConfig;
        blueprint = new Blueprint(address(this), blueprintConfig);
        deploymentConfig.governanceTimelock = blueprint.predictTimelock();
        configHash = StaticConfig.hash(deploymentConfig, block.chainid);
        require(configHash == blueprint.staticConfigHash(), "derived config");
        _deploySuite();
        _requireGovernanceTopology();
        _bootstrapGovernance();
    }

    function testCompleteDeterministicSuiteAndCustodyInvariants() public {
        require(blueprint.deploymentsSealed(), "blueprint open");
        require(
            managerContainer.currentManifestRoot() == Preinstalls.validateComplete(_manifests()), "manifest mismatch"
        );
        require(managerContainer.roleCount() == Predeploys.COUNT, "incomplete roles");
        for (uint256 i = 0; i < Predeploys.COUNT; ++i) {
            bytes32 role = Predeploys.roleAt(i);
            address component = _component(role);
            require(blueprint.deploymentForRole(role) == component, "non-blueprint component");
            require(managerContainer.componentForRole(role) == component, "unmanaged component");
            require(managerContainer.selectorsForRole(role).length == 0, "immutable role migration selector");
            (bool success, bytes memory reason) =
                component.call(abi.encodeCall(UUPSNotUpgradeable.upgradeTo, (address(0x1234))));
            require(!success && reason.length >= 4, "upgrade accepted");
            bytes4 selector;
            assembly ("memory-safe") {
                selector := mload(add(reason, 32))
            }
            require(selector == UUPSNotUpgradeable.UpgradesPermanentlyDisabled.selector, "ambiguous upgrade rejection");
            (success, reason) =
                component.call(abi.encodeCall(UUPSNotUpgradeable.upgradeToAndCall, (address(0x1234), bytes(""))));
            require(!success && reason.length >= 4, "upgrade-and-call accepted");
            assembly ("memory-safe") {
                selector := mload(add(reason, 32))
            }
            require(
                selector == UUPSNotUpgradeable.UpgradesPermanentlyDisabled.selector,
                "ambiguous upgrade-and-call rejection"
            );
        }

        bytes32 assetId = keccak256("USDX");
        _governanceCall(
            address(assetRegistry),
            abi.encodeCall(
                AssetRegistry.registerAsset,
                (assetId, address(token), uint8(6), uint128(1_000_000), uint128(1_000_000_000_000))
            )
        );
        _governanceCall(address(vault), abi.encodeCall(LayerXVault.setSettlementModule, (address(this), true)));
        address depositor = address(0xA11CE);
        uint256 deposited = 1_000_000_000;
        token.mint(depositor, deposited);
        vm.prank(depositor);
        token.approve(address(vault), deposited);
        vm.prank(depositor);
        vault.deposit(assetId, deposited, keccak256("agent-account"));
        bytes32 claimId = keccak256("single-release");
        uint256 released = 400_000_000;
        vault.release(claimId, assetId, depositor, released);
        require(vault.releasedClaim(claimId), "release unrecorded");
        require(token.balanceOf(address(vault)) + vault.totalReleased(assetId) == deposited, "custody conservation");
        require(vault.totalCustodied(assetId) == token.balanceOf(address(vault)), "custody mirror");
        vm.expectPartialRevert(LayerXVault.InvalidRelease.selector);
        vault.release(claimId, assetId, depositor, 1);

        GovernanceTarget target = new GovernanceTarget();
        bytes memory targetCall = abi.encodeCall(GovernanceTarget.setValue, (uint256(77)));
        vm.expectPartialRevert(LayerXTimelock.InvalidOperation.selector);
        vm.prank(FINAL_PROPOSER);
        timelock.schedule(address(target), 0, targetCall, keccak256("blocked"), 2 days);
        bytes memory permissionCall = abi.encodeCall(
            LayerXTimelock.setCallPermission, (address(target), GovernanceTarget.setValue.selector, true)
        );
        _governanceCall(address(timelock), permissionCall);
        _governanceCall(address(target), targetCall);
        require(target.value() == 77, "allowed governance call failed");

        bytes memory failurePermission =
            abi.encodeCall(LayerXTimelock.setCallPermission, (address(target), GovernanceTarget.fail.selector, true));
        _governanceCall(address(timelock), failurePermission);
        bytes memory failureCall = abi.encodeCall(GovernanceTarget.fail, ());
        ScheduledCall memory failure = _schedule(address(target), failureCall);
        vm.warp(block.timestamp + timelock.minDelay());
        vm.expectPartialRevert(LayerXTimelock.CallFailed.selector);
        vm.prank(FINAL_EXECUTOR);
        timelock.execute(failure.target, 0, failure.data, failure.salt, failure.nonce);

        bytes memory initCode = abi.encodePacked(type(IntegrationToken).creationCode, abi.encode(address(this)));
        vm.expectPartialRevert(Blueprint.BlueprintSealed.selector);
        blueprint.deploy(Predeploys.ASSET_REGISTRY, initCode, address(token).codehash);
    }

    function testBootstrapRequiresBothGovernanceDelays() public {
        LayerXTimelock isolated = new LayerXTimelock(
            2 days, 7 days, address(this), address(this), address(this), 1 ether, configHash, release
        );
        GovernanceTarget target = new GovernanceTarget();
        bytes memory permission = abi.encodeCall(
            LayerXTimelock.setCallPermission, (address(target), GovernanceTarget.setValue.selector, true)
        );
        isolated.schedule(address(isolated), 0, permission, keccak256("first-delay"), isolated.minDelay());
        vm.expectPartialRevert(LayerXTimelock.OperationNotReady.selector);
        isolated.execute(address(isolated), 0, permission, keccak256("first-delay"), 0);
        vm.warp(block.timestamp + isolated.minDelay());
        isolated.execute(address(isolated), 0, permission, keccak256("first-delay"), 0);

        bytes memory callData = abi.encodeCall(GovernanceTarget.setValue, (uint256(91)));
        isolated.schedule(address(target), 0, callData, keccak256("second-delay"), isolated.minDelay());
        vm.expectPartialRevert(LayerXTimelock.OperationNotReady.selector);
        isolated.execute(address(target), 0, callData, keccak256("second-delay"), 1);
        vm.warp(block.timestamp + isolated.minDelay());
        isolated.execute(address(target), 0, callData, keccak256("second-delay"), 1);
        require(target.value() == 91, "second governance delay absent");
    }

    function testBootstrapRolesAreFullyTransferredAndRevoked() public view {
        require(timelock.proposer(FINAL_PROPOSER), "final proposer missing");
        require(timelock.executor(FINAL_EXECUTOR), "final executor missing");
        require(timelock.guardian(EMERGENCY_COUNCIL), "final guardian missing");
        require(!timelock.proposer(address(this)), "bootstrap proposer retained");
        require(!timelock.executor(address(this)), "bootstrap executor retained");
        require(!timelock.guardian(address(this)), "bootstrap guardian retained");
    }

    function testRoleTransitionFailureLeavesBlueprintUnsealed() public {
        injectRoleTransitionFailure = true;
        roleTransitionFailureObserved = false;
        setUp();

        require(roleTransitionFailureObserved, "role transition did not fail");
        require(managerContainer.initialized(), "manager not initialized");
        require(managerContainer.migrator() == address(managerMigrator), "migrator missing");
        require(!blueprint.deploymentsSealed(), "failed transition sealed blueprint");
    }

    function testDeployerCannotBypassImmutableTimelockGovernance() public {
        vm.expectPartialRevert(Governed.GovernanceOnly.selector);
        assetRegistry.updateRisk(keccak256("USDX"), 1, 1, true);
        vm.expectPartialRevert(Governed.GovernanceOnly.selector);
        vault.setSettlementModule(address(withdrawalClaims), true);
        vm.expectPartialRevert(GuarantorBond.Unauthorized.selector);
        guarantorBond.setSlashingAuthority(address(challengeManager));
        vm.expectPartialRevert(ManagerUnauthorized.selector);
        managerContainer.setMigrator(address(managerMigrator));
    }

    function testAssetAndNullifierAdministrativeLifecycles() public {
        _prepareSettlement(1_000_000_000);
        bytes32 assetId = keccak256("USDX");
        _governanceCall(
            address(assetRegistry),
            abi.encodeCall(AssetRegistry.updateRisk, (assetId, uint128(2_000_000), uint128(2_000_000_000_000), true))
        );
        vm.prank(EMERGENCY_COUNCIL);
        assetRegistry.emergencyPause(assetId);
        require(assetRegistry.asset(assetId).paused, "emergency pause absent");
        _governanceCall(address(assetRegistry), abi.encodeCall(AssetRegistry.governanceUnpause, (assetId)));
        require(!assetRegistry.asset(assetId).paused, "governance unpause absent");

        bytes32 firstNullifier = keccak256("lifecycle-nullifier-consume");
        bytes32 firstWithdrawal = keccak256("lifecycle-withdrawal-consume");
        bytes32 firstClaim = keccak256("lifecycle-claim-consume");
        nullifierRegistry.reserve(firstNullifier, firstWithdrawal, firstClaim);
        nullifierRegistry.consume(firstNullifier, firstClaim);
        require(
            nullifierRegistry.status(firstNullifier) == WithdrawalNullifierRegistry.Status.Consumed,
            "nullifier not consumed"
        );

        bytes32 secondNullifier = keccak256("lifecycle-nullifier-cancel");
        bytes32 secondWithdrawal = keccak256("lifecycle-withdrawal-cancel");
        bytes32 secondClaim = keccak256("lifecycle-claim-cancel");
        nullifierRegistry.reserve(secondNullifier, secondWithdrawal, secondClaim);
        nullifierRegistry.cancel(secondNullifier, secondClaim);
        require(
            nullifierRegistry.status(secondNullifier) == WithdrawalNullifierRegistry.Status.Cancelled,
            "nullifier not cancelled"
        );
        vm.expectPartialRevert(WithdrawalNullifierRegistry.InvalidTransition.selector);
        nullifierRegistry.consume(secondNullifier, secondClaim);
    }

    function testWithdrawalClaimFinalisesExactlyOnce() public {
        _prepareSettlement(1_000_000_000);
        address recipient = address(0xB0B);
        WithdrawalClaims.Withdrawal memory withdrawal = _withdrawal(recipient, 125_000_000);
        bytes32 stateRoot = withdrawalClaims.withdrawalLeaf(withdrawal);
        (
            bytes32 checkpointHash,
            CanonicalCheckpoint.HeaderCommitments memory header,
            CanonicalCheckpoint.GuarantorAttestation[] memory attestations
        ) = _registerCheckpoint(stateRoot);
        withdrawal.checkpointHash = checkpointHash;
        WithdrawalClaims.StateProof memory proof =
            WithdrawalClaims.StateProof({leafIndex: 0, siblings: new bytes32[](0)});
        bytes32 claimId = withdrawalClaims.queueClaim(
            withdrawal, stateRoot, header.epoch, header.batchNumber, header.dataAvailabilityRoot, proof, attestations
        );
        vm.expectPartialRevert(WithdrawalClaims.ClaimNotReady.selector);
        withdrawalClaims.finaliseClaim(claimId);
        vm.warp(challengeManager.windowClosesAt(checkpointHash));
        withdrawalClaims.finaliseClaim(claimId);
        require(token.balanceOf(recipient) == withdrawal.amount, "withdrawal unpaid");
        require(
            nullifierRegistry.status(withdrawalClaims.withdrawalNullifier(withdrawal))
                == WithdrawalNullifierRegistry.Status.Consumed,
            "withdrawal nullifier open"
        );
        vm.expectPartialRevert(WithdrawalClaims.ClaimNotReady.selector);
        withdrawalClaims.finaliseClaim(claimId);
    }

    function testUpheldChallengeCancelsWithdrawalAndSlashesCertificate() public {
        _prepareSettlement(1_000_000_000);
        WithdrawalClaims.Withdrawal memory withdrawal = _withdrawal(address(0xCA11), 100_000_000);
        bytes32 stateRoot = withdrawalClaims.withdrawalLeaf(withdrawal);
        (
            bytes32 checkpointHash,
            CanonicalCheckpoint.HeaderCommitments memory header,
            CanonicalCheckpoint.GuarantorAttestation[] memory attestations
        ) = _registerCheckpoint(stateRoot);
        withdrawal.checkpointHash = checkpointHash;
        WithdrawalClaims.StateProof memory proof =
            WithdrawalClaims.StateProof({leafIndex: 0, siblings: new bytes32[](0)});
        bytes32 claimId = withdrawalClaims.queueClaim(
            withdrawal, stateRoot, header.epoch, header.batchNumber, header.dataAvailabilityRoot, proof, attestations
        );

        address challenger = address(0xC0FFEE);
        vm.deal(challenger, 2 ether);
        vm.prank(challenger);
        challengeManager.raiseChallenge{value: 1 ether}(
            checkpointHash, CheckpointChallengeManager.Kind.DataAvailability, keccak256("missing-shard")
        );
        _governanceCall(
            address(challengeManager),
            abi.encodeCall(CheckpointChallengeManager.resolveChallenge, (checkpointHash, true))
        );
        require(checkpointRegistry.explicitlyInvalidated(checkpointHash), "checkpoint invalidation not recorded");
        require(!checkpointRegistry.isCanonicalCheckpoint(checkpointHash), "challenged checkpoint remained canonical");
        withdrawalClaims.cancelChallengedClaim(claimId);
        require(
            nullifierRegistry.status(withdrawalClaims.withdrawalNullifier(withdrawal))
                == WithdrawalNullifierRegistry.Status.Cancelled,
            "challenged nullifier active"
        );
        require(guarantorBond.bondRecord(bytes32(uint256(1))).jailed, "first guarantor not slashed");
        require(guarantorBond.bondRecord(bytes32(uint256(2))).jailed, "second guarantor not slashed");
        vm.expectPartialRevert(WithdrawalClaims.ClaimNotReady.selector);
        withdrawalClaims.finaliseClaim(claimId);
    }

    function testRejectedChallengeForfeitsBondToGovernance() public {
        _prepareSettlement(1_000_000_000);
        (bytes32 checkpointHash,,) = _registerCheckpoint(keccak256("unchallenged-state"));
        address challenger = address(0xC0FFEE);
        vm.deal(challenger, 2 ether);
        vm.prank(challenger);
        challengeManager.raiseChallenge{value: 1 ether}(
            checkpointHash, CheckpointChallengeManager.Kind.Fraud, keccak256("invalid-transition")
        );
        uint256 governanceBalance = address(timelock).balance;
        _governanceCall(
            address(challengeManager),
            abi.encodeCall(CheckpointChallengeManager.resolveChallenge, (checkpointHash, false))
        );
        require(address(timelock).balance == governanceBalance + 1 ether, "rejected bond not conserved");
        vm.warp(challengeManager.windowClosesAt(checkpointHash));
        require(challengeManager.claimable(checkpointHash), "rejected checkpoint not claimable");
    }

    function testEmergencyExitUsesLatestCertifiedBalanceExactlyOnce() public {
        _prepareSettlement(1_000_000_000);
        bytes32 account = keccak256("emergency-account");
        bytes32 assetId = keccak256("USDX");
        address recipient = address(0xE911);
        uint128 balance = 300_000_000;
        bytes32 stateRoot = PaxeerWithdrawalCodec.balanceLeaf(account, assetId, balance, recipient);
        (bytes32 checkpointHash,, CanonicalCheckpoint.GuarantorAttestation[] memory attestations) =
            _registerCheckpoint(stateRoot);
        EmergencyExit.ExitClaim memory exitClaim = EmergencyExit.ExitClaim({
            withdrawalId: emergencyExit.requiredWithdrawalId(account, assetId, checkpointHash),
            account: account,
            assetId: assetId,
            finalisedBalance: balance,
            recipient: recipient,
            checkpointHash: checkpointHash
        });
        EmergencyExit.BalanceProof memory proof = EmergencyExit.BalanceProof({leafIndex: 0, siblings: new bytes32[](0)});
        _governanceCall(address(emergencyExit), abi.encodeCall(EmergencyExit.declareEmergency, (false)));
        vm.prank(EMERGENCY_COUNCIL);
        emergencyExit.emergencyCouncilDeclare();
        emergencyExit.executeExit(exitClaim, stateRoot, proof, attestations);
        require(token.balanceOf(recipient) == balance, "emergency balance unpaid");
        vm.expectPartialRevert(EmergencyExit.ExitAlreadyConsumed.selector);
        emergencyExit.executeExit(exitClaim, stateRoot, proof, attestations);
    }

    function testUpheldLatestCheckpointCannotAuthorizeEmergencyExit() public {
        _prepareSettlement(1_000_000_000);
        bytes32 assetId = keccak256("USDX");
        EmergencyCheckpointFixture memory safe =
            _registerEmergencyCheckpoint(assetId, keccak256("safe-emergency-account"), address(0x5AFE), 300_000_000);
        EmergencyCheckpointFixture memory fraudulent = _registerEmergencyCheckpoint(
            assetId, keccak256("fraudulent-emergency-account"), address(0xBAD), 900_000_000
        );

        address challenger = address(0xC0FFEE);
        vm.deal(challenger, 2 ether);
        vm.prank(challenger);
        challengeManager.raiseChallenge{value: 1 ether}(
            fraudulent.checkpointHash, CheckpointChallengeManager.Kind.Fraud, keccak256("invalid-state-root")
        );
        _governanceCall(
            address(challengeManager),
            abi.encodeCall(CheckpointChallengeManager.resolveChallenge, (fraudulent.checkpointHash, true))
        );

        require(checkpointRegistry.explicitlyInvalidated(fraudulent.checkpointHash), "fraud invalidation not recorded");
        require(
            checkpointRegistry.finalisedStateRoot(fraudulent.checkpointHash) == fraudulent.stateRoot,
            "audit history was erased"
        );
        require(
            checkpointRegistry.checkpointAtBatch(checkpointRegistry.finalisedBatchNumber())
                == fraudulent.checkpointHash,
            "registration history was rewritten"
        );
        require(!checkpointRegistry.isCanonicalCheckpoint(fraudulent.checkpointHash), "fraud remained canonical");
        require(checkpointRegistry.isCanonicalCheckpoint(safe.checkpointHash), "safe predecessor was invalidated");
        require(emergencyExit.latestCheckpointHash() == safe.checkpointHash, "exit did not select safe predecessor");
        require(emergencyExit.eligible(), "upheld fraud did not activate emergency exit");

        EmergencyExit.BalanceProof memory proof = EmergencyExit.BalanceProof({leafIndex: 0, siblings: new bytes32[](0)});
        EmergencyExit.ExitClaim memory fraudulentClaim = _exitClaim(fraudulent, assetId);
        vm.expectPartialRevert(EmergencyExit.InvalidExitClaim.selector);
        emergencyExit.executeExit(fraudulentClaim, fraudulent.stateRoot, proof, fraudulent.attestations);

        emergencyExit.executeExit(_exitClaim(safe, assetId), safe.stateRoot, proof, safe.attestations);
        require(token.balanceOf(safe.recipient) == safe.balance, "safe emergency balance unpaid");
        require(token.balanceOf(fraudulent.recipient) == 0, "fraudulent checkpoint paid out");
    }

    function testReserveReconciliationBindsCertifiedLiabilitiesToCustody() public {
        uint128 custody = 1_000_000_000;
        _prepareSettlement(custody);
        ReserveReconciler.LiabilityReport memory report = ReserveReconciler.LiabilityReport({
            assetId: keccak256("USDX"),
            agentMain: 100_000_000,
            escrow: 100_000_000,
            budget: 100_000_000,
            stream: 100_000_000,
            margin: 100_000_000,
            liquidity: 100_000_000,
            insurance: 100_000_000,
            fees: 100_000_000,
            withdrawals: 0,
            otherSystem: 100_000_000,
            reserveMirror: 100_000_000
        });
        bytes32 stateRoot = reserveReconciler.liabilityLeaf(report);
        (bytes32 checkpointHash,, CanonicalCheckpoint.GuarantorAttestation[] memory attestations) =
            _registerCheckpoint(stateRoot);
        ReserveReconciler.StateProof memory proof =
            ReserveReconciler.StateProof({leafIndex: 0, siblings: new bytes32[](0)});
        ReserveReconciler.Reconciliation memory result =
            reserveReconciler.reconcile(checkpointHash, stateRoot, report, proof, attestations);
        require(result.custody == custody, "custody omitted");
        require(result.circulating == 900_000_000, "liability sum wrong");
        require(result.reserveMirror == 100_000_000, "reserve mirror wrong");
    }

    function testGuarantorAdministrativeSlashAndUnbondLifecycles() public {
        _fundGuarantors();
        _governanceCall(address(guarantorBond), abi.encodeCall(GuarantorBond.updateCustodiedValue, (50 ether)));
        address firstSigner = vm.addr(1);
        _governanceCall(
            address(guarantorBond),
            abi.encodeCall(GuarantorBond.removeGuarantor, (bytes32(uint256(1)), uint64(2), uint64(3)))
        );
        vm.prank(firstSigner);
        guarantorBond.beginUnbond(bytes32(uint256(1)), 1 ether);
        vm.prank(firstSigner);
        guarantorBond.cancelUnbond(bytes32(uint256(1)));
        vm.prank(firstSigner);
        guarantorBond.beginUnbond(bytes32(uint256(1)), 1 ether);
        vm.warp(block.timestamp + guarantorBond.unbondingDelay());
        uint256 signerBalance = firstSigner.balance;
        vm.prank(firstSigner);
        guarantorBond.finalizeUnbond(bytes32(uint256(1)));
        require(firstSigner.balance == signerBalance + 1 ether, "unbond not returned");

        _governanceCall(
            address(guarantorBond), abi.encodeCall(GuarantorBond.setUnresolvedSlashing, (bytes32(uint256(2)), true))
        );
        _governanceCall(
            address(guarantorBond), abi.encodeCall(GuarantorBond.setUnresolvedSlashing, (bytes32(uint256(2)), false))
        );
        _governanceCall(
            address(guarantorBond),
            abi.encodeCall(GuarantorBond.setGuarantorJailStatus, (bytes32(uint256(2)), true, uint64(4)))
        );
        require(guarantorBond.bondRecord(bytes32(uint256(2))).jailed, "administrative jail absent");

        GuarantorBond isolated =
            new GuarantorBond(address(this), address(this), 1, 42, 100, 100 ether, 7 days, configHash, release);
        isolated.setSlashingAuthority(address(this));
        address thirdSigner = vm.addr(3);
        isolated.activateGuarantor(bytes32(uint256(3)), thirdSigner, thirdSigner, 1, 1);
        vm.deal(thirdSigner, 3 ether);
        vm.prank(thirdSigner);
        isolated.depositBond{value: 2 ether}(bytes32(uint256(3)));
        isolated.slashForCheckpoint(bytes32(uint256(3)), keccak256("faulted-checkpoint"));
        address payable recipient = payable(address(0x51A5));
        isolated.sweepSlashed(recipient, 2 ether);
        require(recipient.balance == 2 ether && isolated.slashedBalance() == 0, "slash sweep not conserved");
    }

    function _prepareSettlement(uint256 custody) private {
        bytes32 assetId = keccak256("USDX");
        _governanceCall(
            address(assetRegistry),
            abi.encodeCall(
                AssetRegistry.registerAsset,
                (assetId, address(token), uint8(6), uint128(1_000_000), uint128(1_000_000_000_000))
            )
        );
        _governanceCall(
            address(vault), abi.encodeCall(LayerXVault.setSettlementModule, (address(withdrawalClaims), true))
        );
        _governanceCall(address(vault), abi.encodeCall(LayerXVault.setSettlementModule, (address(emergencyExit), true)));
        _governanceCall(address(vault), abi.encodeCall(LayerXVault.setSettlementModule, (address(this), true)));
        _governanceCall(
            address(nullifierRegistry),
            abi.encodeCall(WithdrawalNullifierRegistry.setConsumer, (address(withdrawalClaims), true))
        );
        _governanceCall(
            address(nullifierRegistry),
            abi.encodeCall(WithdrawalNullifierRegistry.setConsumer, (address(emergencyExit), true))
        );
        _governanceCall(
            address(nullifierRegistry), abi.encodeCall(WithdrawalNullifierRegistry.setConsumer, (address(this), true))
        );
        token.mint(address(this), custody);
        token.approve(address(vault), custody);
        vault.deposit(assetId, custody, keccak256("integration-custody-account"));
        _fundGuarantors();
        _governanceCall(
            address(guarantorBond), abi.encodeCall(GuarantorBond.setSlashingAuthority, (address(challengeManager)))
        );
    }

    function _fundGuarantors() private {
        for (uint256 privateKey = 1; privateKey <= 2; ++privateKey) {
            address signer = vm.addr(privateKey);
            _governanceCall(
                address(guarantorBond),
                abi.encodeCall(
                    GuarantorBond.activateGuarantor,
                    (bytes32(privateKey), signer, signer, uint64(1), uint64(privateKey))
                )
            );
            vm.deal(signer, 3 ether);
            vm.prank(signer);
            guarantorBond.depositBond{value: 2 ether}(bytes32(privateKey));
        }
    }

    function _withdrawal(address recipient, uint128 amount)
        private
        pure
        returns (WithdrawalClaims.Withdrawal memory withdrawal)
    {
        withdrawal = WithdrawalClaims.Withdrawal({
            withdrawalId: keccak256("integration-withdrawal"),
            account: keccak256("integration-account"),
            assetId: keccak256("USDX"),
            amount: amount,
            recipient: recipient,
            checkpointHash: bytes32(0)
        });
    }

    function _registerEmergencyCheckpoint(bytes32 assetId, bytes32 account, address recipient, uint128 balance)
        private
        returns (EmergencyCheckpointFixture memory fixture)
    {
        fixture.account = account;
        fixture.recipient = recipient;
        fixture.balance = balance;
        fixture.stateRoot = PaxeerWithdrawalCodec.balanceLeaf(account, assetId, balance, recipient);
        (bytes32 checkpointHash,, CanonicalCheckpoint.GuarantorAttestation[] memory attestations) =
            _registerCheckpoint(fixture.stateRoot);
        fixture.checkpointHash = checkpointHash;
        fixture.attestations = attestations;
    }

    function _exitClaim(EmergencyCheckpointFixture memory fixture, bytes32 assetId)
        private
        view
        returns (EmergencyExit.ExitClaim memory)
    {
        return EmergencyExit.ExitClaim({
            withdrawalId: emergencyExit.requiredWithdrawalId(fixture.account, assetId, fixture.checkpointHash),
            account: fixture.account,
            assetId: assetId,
            finalisedBalance: fixture.balance,
            recipient: fixture.recipient,
            checkpointHash: fixture.checkpointHash
        });
    }

    function _registerCheckpoint(bytes32 stateRoot)
        private
        returns (
            bytes32 digest,
            CanonicalCheckpoint.HeaderCommitments memory header,
            CanonicalCheckpoint.GuarantorAttestation[] memory attestations
        )
    {
        uint256 minimumTimestampMilliseconds = uint256(checkpointRegistry.finalisedTimestamp()) + 1;
        uint256 minimumWallClockSeconds = (minimumTimestampMilliseconds + 999) / 1_000;
        if (block.timestamp < minimumWallClockSeconds) vm.warp(minimumWallClockSeconds);
        require(block.timestamp <= type(uint64).max / 1_000, "test wall clock exceeds milliseconds");
        header = CanonicalCheckpoint.HeaderCommitments({
            protocolVersion: checkpointRegistry.protocolVersion(),
            networkId: checkpointRegistry.networkId(),
            epoch: checkpointRegistry.finalisedEpoch() + 1,
            batchNumber: checkpointRegistry.finalisedBatchNumber() + 1,
            firstSequence: checkpointRegistry.finalisedLastSequence() + 1,
            lastSequence: checkpointRegistry.finalisedLastSequence() + 100,
            previousStateRoot: checkpointRegistry.latestFinalisedStateRoot(),
            resultingStateRoot: stateRoot,
            activityMerkleRoot: keccak256("integration-activity-root"),
            receiptMerkleRoot: keccak256("integration-receipt-root"),
            eventMerkleRoot: keccak256("integration-event-root"),
            dataAvailabilityRoot: keccak256("integration-da-root"),
            oracleRoot: keccak256("integration-oracle-root"),
            timestamp: uint64(block.timestamp * 1_000),
            sequencerId: keccak256("integration-sequencer")
        });
        digest = checkpointRegistry.checkpointHash(header, "");
        attestations = _checkpointAttestations(header, digest);
        checkpointRegistry.registerCheckpoint(header, "", attestations);
        require(
            checkpointRegistry.checkpointGuarantorSetVersion(digest) == guarantorBond.membershipVersion(),
            "checkpoint omitted guarantor-set version"
        );
    }

    function _checkpointAttestations(CanonicalCheckpoint.HeaderCommitments memory header, bytes32 digest)
        private
        returns (CanonicalCheckpoint.GuarantorAttestation[] memory attestations)
    {
        attestations = new CanonicalCheckpoint.GuarantorAttestation[](2);
        for (uint256 i = 0; i < attestations.length; ++i) {
            uint256 privateKey = i + 1;
            attestations[i] = CanonicalCheckpoint.GuarantorAttestation({
                protocolVersion: header.protocolVersion,
                networkId: header.networkId,
                paxeerChainId: uint64(block.chainid),
                settlementContract: address(guarantorBond),
                epoch: header.epoch,
                checkpointId: digest,
                checkpointHash: digest,
                guarantorId: bytes32(privateKey),
                batchNumber: header.batchNumber,
                dataAvailabilityRoot: header.dataAvailabilityRoot,
                replayed: true,
                dataAvailable: true,
                availabilityClassMask: Constants.ALL_AVAILABILITY_CLASSES,
                attestedAt: header.timestamp + 1,
                signer: vm.addr(privateKey),
                r: bytes32(0),
                s: bytes32(0),
                v: 0
            });
            bytes32 attestationDigest = CanonicalCheckpoint.attestationHash(attestations[i]);
            (uint8 v, bytes32 r, bytes32 s) = vm.sign(privateKey, attestationDigest);
            attestations[i].v = v;
            attestations[i].r = r;
            attestations[i].s = s;
        }
    }

    function _deploySuite() private {
        timelock = _deployTimelock();
        assetRegistry = _deployAssetRegistry();
        vault = _deployVault();
        guarantorBond = _deployGuarantorBond();
        checkpointRegistry = _deployCheckpointRegistry();
        challengeManager = _deployChallengeManager();
        nullifierRegistry = _deployNullifierRegistry();
        withdrawalClaims = _deployWithdrawalClaims();
        emergencyExit = _deployEmergencyExit();
        reserveReconciler = _deployReserveReconciler();
        managerContainer = _deployManagerContainer();
        managerMigrator = _deployManagerMigrator();
        custodyTopology = _deployCustodyTopology();
    }

    function _bootstrapGovernance() private {
        address[] memory targets = new address[](19);
        bytes4[] memory selectors = new bytes4[](19);
        uint256 index;
        targets[index] = address(assetRegistry);
        selectors[index++] = AssetRegistry.registerAsset.selector;
        targets[index] = address(assetRegistry);
        selectors[index++] = AssetRegistry.updateRisk.selector;
        targets[index] = address(assetRegistry);
        selectors[index++] = AssetRegistry.governanceUnpause.selector;
        targets[index] = address(vault);
        selectors[index++] = LayerXVault.setSettlementModule.selector;
        targets[index] = address(guarantorBond);
        selectors[index++] = GuarantorBond.updateCustodiedValue.selector;
        targets[index] = address(guarantorBond);
        selectors[index++] = GuarantorBond.setSlashingAuthority.selector;
        targets[index] = address(guarantorBond);
        selectors[index++] = GuarantorBond.activateGuarantor.selector;
        targets[index] = address(guarantorBond);
        selectors[index++] = GuarantorBond.rotateGuarantorSigner.selector;
        targets[index] = address(guarantorBond);
        selectors[index++] = GuarantorBond.removeGuarantor.selector;
        targets[index] = address(guarantorBond);
        selectors[index++] = GuarantorBond.setGuarantorJailStatus.selector;
        targets[index] = address(guarantorBond);
        selectors[index++] = GuarantorBond.setUnresolvedSlashing.selector;
        targets[index] = address(guarantorBond);
        selectors[index++] = GuarantorBond.sweepSlashed.selector;
        targets[index] = address(challengeManager);
        selectors[index++] = CheckpointChallengeManager.resolveChallenge.selector;
        targets[index] = address(nullifierRegistry);
        selectors[index++] = WithdrawalNullifierRegistry.setConsumer.selector;
        targets[index] = address(emergencyExit);
        selectors[index++] = EmergencyExit.declareEmergency.selector;
        targets[index] = address(managerContainer);
        selectors[index++] = ManagerContainer.initialize.selector;
        targets[index] = address(managerContainer);
        selectors[index++] = ManagerContainer.setMigrator.selector;
        targets[index] = address(managerMigrator);
        selectors[index++] = ManagerMigrator.stageMigration.selector;
        targets[index] = address(managerMigrator);
        selectors[index++] = ManagerMigrator.cancelMigration.selector;
        require(index == targets.length, "permission inventory");

        ScheduledCall[] memory permissionCalls = new ScheduledCall[](targets.length);
        for (uint256 i = 0; i < targets.length; ++i) {
            permissionCalls[i] = _schedule(
                address(timelock), abi.encodeCall(LayerXTimelock.setCallPermission, (targets[i], selectors[i], true))
            );
        }
        vm.expectPartialRevert(LayerXTimelock.OperationNotReady.selector);
        timelock.execute(
            permissionCalls[0].target, 0, permissionCalls[0].data, permissionCalls[0].salt, permissionCalls[0].nonce
        );
        vm.warp(block.timestamp + timelock.minDelay());
        for (uint256 i = 0; i < permissionCalls.length; ++i) {
            _execute(permissionCalls[i]);
            require(timelock.callPermission(targets[i], selectors[i]), "permission missing");
        }

        ScheduledCall[] memory initializationCalls = new ScheduledCall[](8);
        initializationCalls[0] = _schedule(
            address(managerContainer), abi.encodeCall(ManagerContainer.initialize, (_manifests(), _allowlists()))
        );
        initializationCalls[1] = _schedule(
            address(managerContainer), abi.encodeCall(ManagerContainer.setMigrator, (address(managerMigrator)))
        );
        initializationCalls[2] =
            _schedule(address(timelock), abi.encodeCall(LayerXTimelock.setRole, (uint8(1), FINAL_PROPOSER, true)));
        initializationCalls[3] =
            _schedule(address(timelock), abi.encodeCall(LayerXTimelock.setRole, (uint8(2), FINAL_EXECUTOR, true)));
        initializationCalls[4] =
            _schedule(address(timelock), abi.encodeCall(LayerXTimelock.setRole, (uint8(3), EMERGENCY_COUNCIL, true)));
        initializationCalls[5] =
            _schedule(address(timelock), abi.encodeCall(LayerXTimelock.setRole, (uint8(1), address(this), false)));
        initializationCalls[6] =
            _schedule(address(timelock), abi.encodeCall(LayerXTimelock.setRole, (uint8(3), address(this), false)));
        initializationCalls[7] =
            _schedule(address(timelock), abi.encodeCall(LayerXTimelock.setRole, (uint8(2), address(this), false)));
        ScheduledCall memory failingRoleCall;
        if (injectRoleTransitionFailure) {
            failingRoleCall =
                _schedule(address(timelock), abi.encodeCall(LayerXTimelock.setRole, (uint8(0), FINAL_PROPOSER, true)));
        }
        vm.expectPartialRevert(LayerXTimelock.OperationNotReady.selector);
        timelock.execute(
            initializationCalls[0].target,
            0,
            initializationCalls[0].data,
            initializationCalls[0].salt,
            initializationCalls[0].nonce
        );
        vm.warp(block.timestamp + timelock.minDelay());
        _execute(initializationCalls[0]);
        _execute(initializationCalls[1]);
        if (injectRoleTransitionFailure) {
            _execute(initializationCalls[2]);
            vm.expectPartialRevert(LayerXTimelock.CallFailed.selector);
            timelock.execute(
                failingRoleCall.target, 0, failingRoleCall.data, failingRoleCall.salt, failingRoleCall.nonce
            );
            roleTransitionFailureObserved = true;
            require(!blueprint.deploymentsSealed(), "failed transition sealed blueprint");
            return;
        }
        for (uint256 i = 2; i < initializationCalls.length; ++i) {
            _execute(initializationCalls[i]);
        }
        require(timelock.proposer(FINAL_PROPOSER), "final proposer missing");
        require(timelock.executor(FINAL_EXECUTOR), "final executor missing");
        require(timelock.guardian(EMERGENCY_COUNCIL), "final guardian missing");
        require(!timelock.proposer(address(this)), "bootstrap proposer retained");
        require(!timelock.executor(address(this)), "bootstrap executor retained");
        require(!timelock.guardian(address(this)), "bootstrap guardian retained");
        require(managerContainer.initialized(), "manager not initialized");
        require(managerContainer.migrator() == address(managerMigrator), "migrator missing");
        for (uint256 i = 0; i < Predeploys.COUNT; ++i) {
            bytes32 role = Predeploys.roleAt(i);
            require(managerContainer.componentForRole(role) == _component(role), "manager topology mismatch");
        }
        _requireGovernanceTopology();
        blueprint.seal();
        require(blueprint.deploymentsSealed(), "blueprint open");
    }

    function _requireGovernanceTopology() private view {
        address governance = address(timelock);
        require(blueprint.governanceTimelock() == governance, "blueprint governance");
        require(blueprint.emergencyCouncil() == EMERGENCY_COUNCIL, "blueprint emergency council");
        require(assetRegistry.governance() == governance, "asset governance");
        require(assetRegistry.emergencyCouncil() == EMERGENCY_COUNCIL, "asset emergency council");
        require(vault.governance() == governance, "vault governance");
        require(vault.emergencyCouncil() == EMERGENCY_COUNCIL, "vault emergency council");
        require(guarantorBond.custodyAuthority() == governance, "bond custody authority");
        require(guarantorBond.membershipAuthority() == governance, "bond membership authority");
        require(challengeManager.governance() == governance, "challenge governance");
        require(challengeManager.emergencyCouncil() == EMERGENCY_COUNCIL, "challenge emergency council");
        require(nullifierRegistry.governance() == governance, "nullifier governance");
        require(nullifierRegistry.emergencyCouncil() == EMERGENCY_COUNCIL, "nullifier emergency council");
        require(emergencyExit.governance() == governance, "exit governance");
        require(emergencyExit.emergencyCouncil() == EMERGENCY_COUNCIL, "exit emergency council");
        require(managerContainer.governanceTimelock() == governance, "manager governance");
        require(managerContainer.emergencyCouncil() == EMERGENCY_COUNCIL, "manager emergency council");
        require(managerMigrator.governanceTimelock() == governance, "migrator governance");
        require(address(vault.assetRegistry()) == address(assetRegistry), "vault registry");
        require(address(checkpointRegistry.guarantorEligibility()) == address(guarantorBond), "checkpoint bond");
        require(address(challengeManager.registry()) == address(checkpointRegistry), "challenge registry");
        require(address(challengeManager.guarantorBond()) == address(guarantorBond), "challenge bond");
        require(address(withdrawalClaims.registry()) == address(checkpointRegistry), "claims registry");
        require(address(withdrawalClaims.challengeManager()) == address(challengeManager), "claims challenge");
        require(address(withdrawalClaims.nullifierRegistry()) == address(nullifierRegistry), "claims nullifier");
        require(address(withdrawalClaims.vault()) == address(vault), "claims vault");
        require(address(emergencyExit.registry()) == address(checkpointRegistry), "exit registry");
        require(address(emergencyExit.challengeManager()) == address(challengeManager), "exit challenge");
        require(address(emergencyExit.nullifierRegistry()) == address(nullifierRegistry), "exit nullifier");
        require(address(emergencyExit.vault()) == address(vault), "exit vault");
        require(address(reserveReconciler.registry()) == address(checkpointRegistry), "reconciler registry");
        require(address(reserveReconciler.vault()) == address(vault), "reconciler vault");
        require(address(reserveReconciler.withdrawalClaims()) == address(withdrawalClaims), "reconciler claims");
        require(address(managerMigrator.container()) == address(managerContainer), "migrator container");
    }

    function _governanceCall(address target, bytes memory data) private returns (bytes memory result) {
        ScheduledCall memory scheduled = _schedule(target, data);
        vm.warp(block.timestamp + timelock.minDelay());
        return _execute(scheduled);
    }

    function _schedule(address target, bytes memory data) private returns (ScheduledCall memory scheduled) {
        uint256 saltNonce = governanceSaltNonce++;
        scheduled = ScheduledCall({
            target: target,
            data: data,
            salt: keccak256(abi.encode("integration-governance", saltNonce, target, keccak256(data))),
            nonce: timelock.operationNonce()
        });
        uint64 delay = timelock.minDelay();
        if (timelock.proposer(FINAL_PROPOSER)) vm.prank(FINAL_PROPOSER);
        timelock.schedule(target, 0, data, scheduled.salt, delay);
    }

    function _execute(ScheduledCall memory scheduled) private returns (bytes memory result) {
        if (timelock.executor(FINAL_EXECUTOR)) vm.prank(FINAL_EXECUTOR);
        return timelock.execute(scheduled.target, 0, scheduled.data, scheduled.salt, scheduled.nonce);
    }

    function _deployTimelock() private returns (LayerXTimelock result) {
        bytes memory arguments = abi.encode(
            uint64(2 days),
            uint64(7 days),
            address(this),
            address(this),
            address(this),
            uint256(10 ether),
            configHash,
            release
        );
        LayerXTimelock runtimeReference = new LayerXTimelock(
            2 days, 7 days, address(this), address(this), address(this), 10 ether, configHash, release
        );
        result = LayerXTimelock(
            payable(blueprint.deployTimelock(
                    abi.encodePacked(type(LayerXTimelock).creationCode, arguments), address(runtimeReference).codehash
                ))
        );
        require(address(result) == blueprint.predictTimelock(), "timelock prediction mismatch");
    }

    function _deployAssetRegistry() private returns (AssetRegistry result) {
        bytes memory arguments = abi.encode(address(timelock), EMERGENCY_COUNCIL, configHash, release);
        AssetRegistry runtimeReference = new AssetRegistry(address(timelock), EMERGENCY_COUNCIL, configHash, release);
        result = AssetRegistry(
            _deploy(
                Predeploys.ASSET_REGISTRY,
                abi.encodePacked(type(AssetRegistry).creationCode, arguments),
                address(runtimeReference).codehash
            )
        );
    }

    function _deployVault() private returns (LayerXVault result) {
        bytes memory arguments = abi.encode(assetRegistry, address(timelock), EMERGENCY_COUNCIL, configHash, release);
        LayerXVault runtimeReference =
            new LayerXVault(assetRegistry, address(timelock), EMERGENCY_COUNCIL, configHash, release);
        result = LayerXVault(
            _deploy(
                Predeploys.VAULT,
                abi.encodePacked(type(LayerXVault).creationCode, arguments),
                address(runtimeReference).codehash
            )
        );
    }

    function _deployGuarantorBond() private returns (GuarantorBond result) {
        bytes memory arguments = abi.encode(
            address(timelock),
            address(timelock),
            uint16(1),
            uint32(42),
            uint32(100),
            uint256(100 ether),
            uint64(7 days),
            configHash,
            release
        );
        GuarantorBond runtimeReference =
            new GuarantorBond(address(timelock), address(timelock), 1, 42, 100, 100 ether, 7 days, configHash, release);
        result = GuarantorBond(
            payable(_deploy(
                    Predeploys.GUARANTOR_BOND,
                    abi.encodePacked(type(GuarantorBond).creationCode, arguments),
                    address(runtimeReference).codehash
                ))
        );
    }

    function _deployCheckpointRegistry() private returns (CheckpointRegistry result) {
        bytes memory arguments = abi.encode(
            guarantorBond,
            uint16(1),
            uint32(42),
            uint16(2),
            uint16(4),
            uint64(1 hours),
            uint64(5 minutes),
            GENESIS_RECEIPT_ROOT,
            configHash,
            release
        );
        CheckpointRegistry runtimeReference = new CheckpointRegistry(
            guarantorBond, 1, 42, 2, 4, 1 hours, 5 minutes, GENESIS_RECEIPT_ROOT, configHash, release
        );
        result = CheckpointRegistry(
            _deploy(
                Predeploys.CHECKPOINT_REGISTRY,
                abi.encodePacked(type(CheckpointRegistry).creationCode, arguments),
                address(runtimeReference).codehash
            )
        );
    }

    function _deployChallengeManager() private returns (CheckpointChallengeManager result) {
        bytes memory arguments = abi.encode(
            checkpointRegistry,
            guarantorBond,
            address(timelock),
            EMERGENCY_COUNCIL,
            uint64(7 days),
            uint128(1 ether),
            configHash,
            release
        );
        CheckpointChallengeManager runtimeReference = new CheckpointChallengeManager(
            checkpointRegistry,
            guarantorBond,
            address(timelock),
            EMERGENCY_COUNCIL,
            7 days,
            1 ether,
            configHash,
            release
        );
        result = CheckpointChallengeManager(
            _deploy(
                Predeploys.CHALLENGE_MANAGER,
                abi.encodePacked(type(CheckpointChallengeManager).creationCode, arguments),
                address(runtimeReference).codehash
            )
        );
    }

    function _deployNullifierRegistry() private returns (WithdrawalNullifierRegistry result) {
        bytes memory arguments = abi.encode(address(timelock), EMERGENCY_COUNCIL, configHash, release);
        WithdrawalNullifierRegistry runtimeReference =
            new WithdrawalNullifierRegistry(address(timelock), EMERGENCY_COUNCIL, configHash, release);
        result = WithdrawalNullifierRegistry(
            _deploy(
                Predeploys.NULLIFIER_REGISTRY,
                abi.encodePacked(type(WithdrawalNullifierRegistry).creationCode, arguments),
                address(runtimeReference).codehash
            )
        );
    }

    function _deployWithdrawalClaims() private returns (WithdrawalClaims result) {
        bytes memory arguments =
            abi.encode(checkpointRegistry, challengeManager, nullifierRegistry, vault, configHash, release);
        WithdrawalClaims runtimeReference =
            new WithdrawalClaims(checkpointRegistry, challengeManager, nullifierRegistry, vault, configHash, release);
        result = WithdrawalClaims(
            _deploy(
                Predeploys.WITHDRAWAL_CLAIMS,
                abi.encodePacked(type(WithdrawalClaims).creationCode, arguments),
                address(runtimeReference).codehash
            )
        );
    }

    function _deployEmergencyExit() private returns (EmergencyExit result) {
        bytes memory arguments = abi.encode(
            checkpointRegistry,
            challengeManager,
            nullifierRegistry,
            vault,
            address(timelock),
            EMERGENCY_COUNCIL,
            uint64(1 days),
            configHash,
            release
        );
        EmergencyExit runtimeReference = new EmergencyExit(
            checkpointRegistry,
            challengeManager,
            nullifierRegistry,
            vault,
            address(timelock),
            EMERGENCY_COUNCIL,
            1 days,
            configHash,
            release
        );
        result = EmergencyExit(
            _deploy(
                Predeploys.EMERGENCY_EXIT,
                abi.encodePacked(type(EmergencyExit).creationCode, arguments),
                address(runtimeReference).codehash
            )
        );
    }

    function _deployReserveReconciler() private returns (ReserveReconciler result) {
        bytes memory arguments = abi.encode(checkpointRegistry, vault, withdrawalClaims, configHash, release);
        ReserveReconciler runtimeReference =
            new ReserveReconciler(checkpointRegistry, vault, withdrawalClaims, configHash, release);
        result = ReserveReconciler(
            _deploy(
                Predeploys.RESERVE_RECONCILER,
                abi.encodePacked(type(ReserveReconciler).creationCode, arguments),
                address(runtimeReference).codehash
            )
        );
    }

    function _deployManagerContainer() private returns (ManagerContainer result) {
        StaticConfig.Config memory config = deploymentConfig;
        bytes memory arguments = abi.encode(config);
        ManagerContainer runtimeReference = new ManagerContainer(config);
        result = ManagerContainer(
            _deploy(
                Predeploys.CONTRACTS_MANAGER,
                abi.encodePacked(type(ManagerContainer).creationCode, arguments),
                address(runtimeReference).codehash
            )
        );
    }

    function _deployManagerMigrator() private returns (ManagerMigrator result) {
        bytes memory arguments = abi.encode(
            managerContainer,
            address(timelock),
            address(this),
            uint64(1 days),
            uint64(7 days),
            uint64(1_000_000),
            uint256(1 ether)
        );
        ManagerMigrator runtimeReference =
            new ManagerMigrator(managerContainer, address(timelock), address(this), 1 days, 7 days, 1_000_000, 1 ether);
        result = ManagerMigrator(
            payable(_deploy(
                    Predeploys.MANAGER_MIGRATOR,
                    abi.encodePacked(type(ManagerMigrator).creationCode, arguments),
                    address(runtimeReference).codehash
                ))
        );
    }

    function _deployCustodyTopology() private returns (LayerXCustody result) {
        bytes memory arguments = abi.encode(
            checkpointRegistry,
            guarantorBond,
            vault,
            challengeManager,
            nullifierRegistry,
            withdrawalClaims,
            configHash,
            release
        );
        LayerXCustody runtimeReference = new LayerXCustody(
            checkpointRegistry,
            guarantorBond,
            vault,
            challengeManager,
            nullifierRegistry,
            withdrawalClaims,
            configHash,
            release
        );
        result = LayerXCustody(
            _deploy(
                Predeploys.CUSTODY_TOPOLOGY,
                abi.encodePacked(type(LayerXCustody).creationCode, arguments),
                address(runtimeReference).codehash
            )
        );
    }

    function _deploy(bytes32 role, bytes memory initCode, bytes32 runtimeHash) private returns (address component) {
        component = blueprint.deploy(role, initCode, runtimeHash);
        require(component == blueprint.predict(role, keccak256(initCode)), "prediction mismatch");
    }

    function _manifests() private view returns (Preinstalls.ComponentManifest[] memory manifests) {
        manifests = new Preinstalls.ComponentManifest[](Predeploys.COUNT);
        for (uint256 i = 0; i < manifests.length; ++i) {
            bytes32 role = Predeploys.roleAt(i);
            address component = _component(role);
            ILayerXComponent attested = ILayerXComponent(component);
            manifests[i] = Preinstalls.ComponentManifest({
                role: role,
                component: component,
                interfaceId: Preinstalls.interfaceId(),
                runtimeCodeHash: component.codehash,
                configHash: attested.staticConfigHash(),
                release: attested.releaseVersion(),
                storageLayout: attested.storageLayoutVersion()
            });
        }
    }

    function _allowlists() private pure returns (bytes4[][] memory lists) {
        lists = new bytes4[][](Predeploys.COUNT);
        for (uint256 i = 0; i < lists.length; ++i) {
            lists[i] = new bytes4[](0);
        }
    }

    function _component(bytes32 role) private view returns (address) {
        if (role == Predeploys.TIMELOCK) return address(timelock);
        if (role == Predeploys.ASSET_REGISTRY) return address(assetRegistry);
        if (role == Predeploys.VAULT) return address(vault);
        if (role == Predeploys.GUARANTOR_BOND) return address(guarantorBond);
        if (role == Predeploys.CHECKPOINT_REGISTRY) {
            return address(checkpointRegistry);
        }
        if (role == Predeploys.CHALLENGE_MANAGER) {
            return address(challengeManager);
        }
        if (role == Predeploys.NULLIFIER_REGISTRY) {
            return address(nullifierRegistry);
        }
        if (role == Predeploys.WITHDRAWAL_CLAIMS) {
            return address(withdrawalClaims);
        }
        if (role == Predeploys.EMERGENCY_EXIT) return address(emergencyExit);
        if (role == Predeploys.RESERVE_RECONCILER) {
            return address(reserveReconciler);
        }
        if (role == Predeploys.CONTRACTS_MANAGER) {
            return address(managerContainer);
        }
        if (role == Predeploys.MANAGER_MIGRATOR) {
            return address(managerMigrator);
        }
        if (role == Predeploys.CUSTODY_TOPOLOGY) {
            return address(custodyTopology);
        }
        revert("unknown role");
    }
}
