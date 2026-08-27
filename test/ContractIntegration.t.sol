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
import {StaticConfig} from "../contracts/config/StaticConfig.sol";
import {Features} from "../contracts/config/Features.sol";
import {Constants} from "../contracts/libraries/Constants.sol";
import {CanonicalCheckpoint} from "../contracts/libraries/CanonicalCheckpoint.sol";
import {MessageTypes} from "../contracts/libraries/MessageTypes.sol";
import {PaxeerWithdrawalCodec} from "../contracts/libraries/PaxeerWithdrawalCodec.sol";
import {SemverComp} from "../contracts/libraries/SemverComp.sol";
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

    IntegrationVm private constant vm = IntegrationVm(address(uint160(uint256(keccak256("hevm cheat code")))));

    bytes32 private constant GENESIS = keccak256("integration-genesis");
    address private constant EMERGENCY_COUNCIL = address(0xEC01);

    uint192 private release;
    bytes32 private configHash;
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
        StaticConfig.Config memory config = StaticConfig.Config({
            chainId: block.chainid,
            protocolVersion: Constants.PROTOCOL_VERSION,
            releaseVersion: release,
            governanceTimelock: address(this),
            emergencyCouncil: EMERGENCY_COUNCIL,
            genesisStateRoot: GENESIS,
            challengeWindow: 7 days,
            checkpointLivenessBound: 1 days,
            enabledFeatures: Features.ERC20_CUSTODY | Features.CHECKPOINT_CHALLENGES | Features.WITHDRAWAL_CLAIMS
                | Features.EMERGENCY_EXIT | Features.RESERVE_RECONCILIATION,
            assetDefinitionsRoot: StaticConfig.hashAssets(assets)
        });
        configHash = StaticConfig.hash(config, block.chainid);
        blueprint = new Blueprint(address(this), configHash, release);
        _deploySuite();
        Preinstalls.ComponentManifest[] memory manifests = _manifests();
        managerContainer.initialize(manifests, _allowlists());
        managerContainer.setMigrator(address(managerMigrator));
        blueprint.seal();
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
        assetRegistry.registerAsset(assetId, address(token), 6, 1_000_000, 1_000_000_000_000);
        vault.setSettlementModule(address(this), true);
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
        timelock.schedule(address(target), 0, targetCall, keccak256("blocked"), 2 days);
        bytes memory permissionCall = abi.encodeCall(
            LayerXTimelock.setCallPermission, (address(target), GovernanceTarget.setValue.selector, true)
        );
        bytes32 permissionId = timelock.schedule(address(timelock), 0, permissionCall, keccak256("permission"), 2 days);
        vm.warp(timelock.readyAt(permissionId));
        timelock.execute(address(timelock), 0, permissionCall, keccak256("permission"), 0);
        bytes32 targetId = timelock.schedule(address(target), 0, targetCall, keccak256("allowed"), 2 days);
        vm.warp(timelock.readyAt(targetId));
        timelock.execute(address(target), 0, targetCall, keccak256("allowed"), 1);
        require(target.value() == 77, "allowed governance call failed");

        bytes memory failurePermission =
            abi.encodeCall(LayerXTimelock.setCallPermission, (address(target), GovernanceTarget.fail.selector, true));
        bytes32 failurePermissionId =
            timelock.schedule(address(timelock), 0, failurePermission, keccak256("failure-permission"), 2 days);
        vm.warp(timelock.readyAt(failurePermissionId));
        timelock.execute(address(timelock), 0, failurePermission, keccak256("failure-permission"), 2);
        bytes memory failureCall = abi.encodeCall(GovernanceTarget.fail, ());
        bytes32 failureId = timelock.schedule(address(target), 0, failureCall, keccak256("bounded-failure"), 2 days);
        vm.warp(timelock.readyAt(failureId));
        vm.expectPartialRevert(LayerXTimelock.CallFailed.selector);
        timelock.execute(address(target), 0, failureCall, keccak256("bounded-failure"), 3);

        bytes memory initCode = abi.encodePacked(type(IntegrationToken).creationCode, abi.encode(address(this)));
        vm.expectPartialRevert(Blueprint.BlueprintSealed.selector);
        blueprint.deploy(Predeploys.ASSET_REGISTRY, initCode, address(token).codehash);
    }

    function testAssetAndNullifierAdministrativeLifecycles() public {
        _prepareSettlement(1_000_000_000);
        bytes32 assetId = keccak256("USDX");
        assetRegistry.updateRisk(assetId, 2_000_000, 2_000_000_000_000, true);
        vm.prank(EMERGENCY_COUNCIL);
        assetRegistry.emergencyPause(assetId);
        require(assetRegistry.asset(assetId).paused, "emergency pause absent");
        assetRegistry.governanceUnpause(assetId);
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
        challengeManager.resolveChallenge(checkpointHash, true);
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
        uint256 governanceBalance = address(this).balance;
        challengeManager.resolveChallenge(checkpointHash, false);
        require(address(this).balance == governanceBalance + 1 ether, "rejected bond not conserved");
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
        emergencyExit.declareEmergency(false);
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
        challengeManager.resolveChallenge(fraudulent.checkpointHash, true);

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
        vm.expectPartialRevert(EmergencyExit.InvalidExitClaim.selector);
        emergencyExit.executeExit(_exitClaim(fraudulent, assetId), fraudulent.stateRoot, proof, fraudulent.attestations);

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
        guarantorBond.updateCustodiedValue(50 ether);
        address firstSigner = vm.addr(1);
        guarantorBond.removeGuarantor(bytes32(uint256(1)), 2, 3);
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

        guarantorBond.setUnresolvedSlashing(bytes32(uint256(2)), true);
        guarantorBond.setUnresolvedSlashing(bytes32(uint256(2)), false);
        guarantorBond.setGuarantorJailStatus(bytes32(uint256(2)), true, 4);
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
        assetRegistry.registerAsset(assetId, address(token), 6, 1_000_000, 1_000_000_000_000);
        vault.setSettlementModule(address(withdrawalClaims), true);
        vault.setSettlementModule(address(emergencyExit), true);
        vault.setSettlementModule(address(this), true);
        nullifierRegistry.setConsumer(address(withdrawalClaims), true);
        nullifierRegistry.setConsumer(address(emergencyExit), true);
        nullifierRegistry.setConsumer(address(this), true);
        token.mint(address(this), custody);
        token.approve(address(vault), custody);
        vault.deposit(assetId, custody, keccak256("integration-custody-account"));
        _fundGuarantors();
        guarantorBond.setSlashingAuthority(address(challengeManager));
    }

    function _fundGuarantors() private {
        for (uint256 privateKey = 1; privateKey <= 2; ++privateKey) {
            address signer = vm.addr(privateKey);
            guarantorBond.activateGuarantor(bytes32(privateKey), signer, signer, 1, uint64(privateKey));
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
            payable(_deploy(
                    Predeploys.TIMELOCK,
                    abi.encodePacked(type(LayerXTimelock).creationCode, arguments),
                    address(runtimeReference).codehash
                ))
        );
    }

    function _deployAssetRegistry() private returns (AssetRegistry result) {
        bytes memory arguments = abi.encode(address(this), EMERGENCY_COUNCIL, configHash, release);
        AssetRegistry runtimeReference = new AssetRegistry(address(this), EMERGENCY_COUNCIL, configHash, release);
        result = AssetRegistry(
            _deploy(
                Predeploys.ASSET_REGISTRY,
                abi.encodePacked(type(AssetRegistry).creationCode, arguments),
                address(runtimeReference).codehash
            )
        );
    }

    function _deployVault() private returns (LayerXVault result) {
        bytes memory arguments = abi.encode(assetRegistry, address(this), EMERGENCY_COUNCIL, configHash, release);
        LayerXVault runtimeReference =
            new LayerXVault(assetRegistry, address(this), EMERGENCY_COUNCIL, configHash, release);
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
            address(this),
            address(this),
            uint16(1),
            uint32(42),
            uint32(100),
            uint256(100 ether),
            uint64(7 days),
            configHash,
            release
        );
        GuarantorBond runtimeReference =
            new GuarantorBond(address(this), address(this), 1, 42, 100, 100 ether, 7 days, configHash, release);
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
            GENESIS,
            configHash,
            release
        );
        CheckpointRegistry runtimeReference =
            new CheckpointRegistry(guarantorBond, 1, 42, 2, 4, 1 hours, 5 minutes, GENESIS, configHash, release);
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
            address(this),
            EMERGENCY_COUNCIL,
            uint64(7 days),
            uint128(1 ether),
            configHash,
            release
        );
        CheckpointChallengeManager runtimeReference = new CheckpointChallengeManager(
            checkpointRegistry, guarantorBond, address(this), EMERGENCY_COUNCIL, 7 days, 1 ether, configHash, release
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
        bytes memory arguments = abi.encode(address(this), EMERGENCY_COUNCIL, configHash, release);
        WithdrawalNullifierRegistry runtimeReference =
            new WithdrawalNullifierRegistry(address(this), EMERGENCY_COUNCIL, configHash, release);
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
            address(this),
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
            address(this),
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
        bytes memory arguments = abi.encode(address(this), configHash, release);
        ManagerContainer runtimeReference = new ManagerContainer(address(this), configHash, release);
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
            address(this),
            address(this),
            uint64(1 days),
            uint64(7 days),
            uint64(1_000_000),
            uint256(1 ether)
        );
        ManagerMigrator runtimeReference =
            new ManagerMigrator(managerContainer, address(this), address(this), 1 days, 7 days, 1_000_000, 1 ether);
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
            lists[i] = new bytes4[](1);
            lists[i][0] = _selectorForRole(Predeploys.roleAt(i));
        }
    }

    function _selectorForRole(bytes32 role) private pure returns (bytes4) {
        if (role == Predeploys.TIMELOCK) {
            return LayerXTimelock.setCallPermission.selector;
        }
        if (role == Predeploys.ASSET_REGISTRY) {
            return AssetRegistry.updateRisk.selector;
        }
        if (role == Predeploys.VAULT) {
            return LayerXVault.setSettlementModule.selector;
        }
        if (role == Predeploys.GUARANTOR_BOND) {
            return GuarantorBond.updateCustodiedValue.selector;
        }
        if (role == Predeploys.CHECKPOINT_REGISTRY) {
            return CheckpointRegistry.registerCheckpoint.selector;
        }
        if (role == Predeploys.CHALLENGE_MANAGER) {
            return CheckpointChallengeManager.resolveChallenge.selector;
        }
        if (role == Predeploys.NULLIFIER_REGISTRY) {
            return WithdrawalNullifierRegistry.setConsumer.selector;
        }
        if (role == Predeploys.WITHDRAWAL_CLAIMS) {
            return WithdrawalClaims.finaliseClaim.selector;
        }
        if (role == Predeploys.EMERGENCY_EXIT) {
            return EmergencyExit.declareEmergency.selector;
        }
        if (role == Predeploys.RESERVE_RECONCILER) {
            return ReserveReconciler.reconcile.selector;
        }
        if (role == Predeploys.CONTRACTS_MANAGER) {
            return ManagerContainer.completeMigration.selector;
        }
        if (role == Predeploys.MANAGER_MIGRATOR) {
            return ManagerMigrator.cancelMigration.selector;
        }
        if (role == Predeploys.CUSTODY_TOPOLOGY) {
            return UUPSNotUpgradeable.upgradeTo.selector;
        }
        revert("unknown role");
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
