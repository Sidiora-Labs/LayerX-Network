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
import {PaxeerBetaDeploymentValidator} from "../contracts/deployment/PaxeerBetaDeploymentValidator.sol";
import {ManagerContainer} from "../contracts/manager/ManagerContainer.sol";
import {ManagerMigrator} from "../contracts/manager/ManagerMigrator.sol";
import {StaticConfig} from "../contracts/config/StaticConfig.sol";
import {Constants} from "../contracts/libraries/Constants.sol";

interface PaxeerBetaVm {
    function addr(uint256 privateKey) external returns (address);
    function envUint(string calldata name) external returns (uint256);
    function startBroadcast(uint256 privateKey) external;
    function stopBroadcast() external;
}

contract PaxeerBetaDeploy {
    PaxeerBetaVm private constant vm = PaxeerBetaVm(address(uint160(uint256(keccak256("hevm cheat code")))));

    error InvalidPhase();
    error InvalidDeploymentState();
    uint256 private broadcastKey;

    struct Addresses {
        address blueprint;
        address timelock;
        address assetRegistry;
        address vault;
        address guarantorBond;
        address checkpointRegistry;
        address challengeManager;
        address nullifierRegistry;
        address withdrawalClaims;
        address emergencyExit;
        address reserveReconciler;
        address managerContainer;
        address managerMigrator;
        address custodyTopology;
    }

    struct ScheduledCall {
        address target;
        bytes data;
        bytes32 salt;
        uint256 nonce;
    }

    event BetaSuiteDeployed(bytes32 indexed configHash, address indexed blueprint, Addresses addresses);
    event BetaGovernancePhase(bytes32 indexed phase, address indexed blueprint, uint256 firstNonce, uint256 callCount);
    event BetaDeploymentComplete(bytes32 indexed deploymentId, address indexed blueprint, Addresses addresses);
    event BetaBondFunded(bytes32 indexed guarantorId, address indexed controller, uint256 amount);

    function predictGuarantorBond(
        PaxeerBetaDeploymentValidator.Input calldata input,
        bytes calldata descriptor,
        bytes calldata registrationRequest
    ) external returns (address) {
        return _predictGuarantorBond(input, descriptor, registrationRequest, Constants.PROTOCOL_VERSION);
    }

    function predictGuarantorBondForProtocol(
        PaxeerBetaDeploymentValidator.Input calldata input,
        bytes calldata descriptor,
        bytes calldata registrationRequest,
        uint16 selectedProtocolVersion
    ) external returns (address) {
        return _predictGuarantorBond(input, descriptor, registrationRequest, selectedProtocolVersion);
    }

    function _predictGuarantorBond(
        PaxeerBetaDeploymentValidator.Input calldata input,
        bytes calldata descriptor,
        bytes calldata registrationRequest,
        uint16 selectedProtocolVersion
    ) private returns (address) {
        PaxeerBetaDeploymentValidator.GenesisArtifacts memory genesis =
            PaxeerBetaDeploymentValidator.decodeAndCrossCheckGenesis(descriptor, registrationRequest);
        (uint192 release, StaticConfig.Config memory config) =
            PaxeerBetaDeploymentValidator.validateInputForProtocol(input, genesis, selectedProtocolVersion);
        uint256 key = vm.envUint("EVM_WALLET_PRIVATE_KEY");
        if (vm.addr(key) != input.bootstrapOperator) revert InvalidDeploymentState();
        vm.startBroadcast(key);
        broadcastKey = key;
        Blueprint blueprint = new Blueprint(input.bootstrapOperator, config);
        Addresses memory addresses;
        addresses.blueprint = address(blueprint);
        addresses.timelock = address(_deployTimelock(blueprint, input, release));
        config.governanceTimelock = addresses.timelock;
        bytes32 configHash = StaticConfig.hashForProtocol(config, block.chainid, selectedProtocolVersion);
        addresses.assetRegistry = _assetRegistry(blueprint, addresses, input, configHash, release);
        addresses.vault = _vault(blueprint, addresses, input, configHash, release);
        addresses.guarantorBond =
            _bond(blueprint, addresses, input, genesis.networkId, configHash, release, selectedProtocolVersion);
        vm.stopBroadcast();
        return addresses.guarantorBond;
    }

    function deploy(
        PaxeerBetaDeploymentValidator.Input calldata input,
        PaxeerBetaDeploymentValidator.GuarantorInput[] calldata guarantors,
        bytes calldata descriptor,
        bytes calldata registrationRequest
    ) external returns (Addresses memory addresses) {
        return _deploySuite(input, guarantors, descriptor, registrationRequest, Constants.PROTOCOL_VERSION);
    }

    function deployForProtocol(
        PaxeerBetaDeploymentValidator.Input calldata input,
        PaxeerBetaDeploymentValidator.GuarantorInput[] calldata guarantors,
        bytes calldata descriptor,
        bytes calldata registrationRequest,
        uint16 selectedProtocolVersion
    ) external returns (Addresses memory addresses) {
        return _deploySuite(input, guarantors, descriptor, registrationRequest, selectedProtocolVersion);
    }

    function _deploySuite(
        PaxeerBetaDeploymentValidator.Input calldata input,
        PaxeerBetaDeploymentValidator.GuarantorInput[] calldata guarantors,
        bytes calldata descriptor,
        bytes calldata registrationRequest,
        uint16 selectedProtocolVersion
    ) private returns (Addresses memory addresses) {
        PaxeerBetaDeploymentValidator.GenesisArtifacts memory genesis =
            PaxeerBetaDeploymentValidator.decodeAndCrossCheckGenesis(descriptor, registrationRequest);
        (uint192 release, StaticConfig.Config memory config) =
            PaxeerBetaDeploymentValidator.validateInputForProtocol(input, genesis, selectedProtocolVersion);
        uint256 key = vm.envUint("EVM_WALLET_PRIVATE_KEY");
        if (vm.addr(key) != input.bootstrapOperator) revert InvalidDeploymentState();
        vm.startBroadcast(key);
        broadcastKey = key;
        Blueprint blueprint = new Blueprint(input.bootstrapOperator, config);
        addresses.blueprint = address(blueprint);
        addresses.timelock = address(_deployTimelock(blueprint, input, release));
        config.governanceTimelock = addresses.timelock;
        bytes32 configHash = StaticConfig.hashForProtocol(config, block.chainid, selectedProtocolVersion);
        addresses.assetRegistry = _assetRegistry(blueprint, addresses, input, configHash, release);
        addresses.vault = _vault(blueprint, addresses, input, configHash, release);
        addresses.guarantorBond =
            _bond(blueprint, addresses, input, genesis.networkId, configHash, release, selectedProtocolVersion);
        addresses.checkpointRegistry =
            _checkpoint(blueprint, addresses, input, genesis, configHash, release, selectedProtocolVersion);
        addresses.challengeManager = _challenge(blueprint, addresses, input, configHash, release);
        addresses.nullifierRegistry = _nullifier(blueprint, input, configHash, release);
        addresses.withdrawalClaims = _claims(blueprint, addresses, configHash, release);
        addresses.emergencyExit = _exit(blueprint, addresses, input, configHash, release);
        addresses.reserveReconciler = _reconciler(blueprint, addresses, configHash, release);
        addresses.managerContainer = _manager(blueprint, config, release);
        addresses.managerMigrator = _migrator(blueprint, addresses, input, release);
        addresses.custodyTopology = _topology(blueprint, addresses, configHash, release);
        PaxeerBetaDeploymentValidator.validateGuarantors(guarantors, addresses.guarantorBond);
        uint256 firstNonce = LayerXTimelock(payable(addresses.timelock)).operationNonce();
        _schedulePermissions(addresses, input.timelockDelay);
        emit BetaSuiteDeployed(configHash, address(blueprint), addresses);
        emit BetaGovernancePhase("PERMISSIONS_SCHEDULED", address(blueprint), firstNonce, 21);
        vm.stopBroadcast();
    }

    function executePermissionsAndScheduleGenesis(
        Addresses calldata addresses,
        PaxeerBetaDeploymentValidator.Input calldata input,
        PaxeerBetaDeploymentValidator.GuarantorInput[] calldata guarantors,
        uint256 permissionStartNonce
    ) external {
        _requireBootstrap(addresses, input);
        uint256 key = vm.envUint("EVM_WALLET_PRIVATE_KEY");
        vm.startBroadcast(key);
        (address[] memory targets, bytes4[] memory selectors) = _permissions(addresses);
        LayerXTimelock timelock = LayerXTimelock(payable(addresses.timelock));
        for (uint256 i = 0; i < targets.length; ++i) {
            bytes memory data = abi.encodeCall(LayerXTimelock.setCallPermission, (targets[i], selectors[i], true));
            _execute(
                timelock,
                addresses.timelock,
                data,
                _salt("PERMISSION", i, addresses.timelock, data),
                permissionStartNonce + i
            );
            if (!timelock.callPermission(targets[i], selectors[i])) revert InvalidDeploymentState();
        }
        uint256 firstNonce = timelock.operationNonce();
        _scheduleGenesis(addresses, input, guarantors);
        emit BetaGovernancePhase("GENESIS_SCHEDULED", addresses.blueprint, firstNonce, 12 + guarantors.length);
        vm.stopBroadcast();
    }

    function executeGenesisActivation(
        Addresses calldata addresses,
        PaxeerBetaDeploymentValidator.Input calldata input,
        PaxeerBetaDeploymentValidator.GuarantorInput[] calldata guarantors,
        uint256 genesisStartNonce
    ) external {
        _requireBootstrap(addresses, input);
        uint256 key = vm.envUint("EVM_WALLET_PRIVATE_KEY");
        vm.startBroadcast(key);
        ScheduledCall[] memory calls = _genesisCalls(addresses, input, guarantors);
        LayerXTimelock timelock = LayerXTimelock(payable(addresses.timelock));
        uint256 activationEnd = 4 + guarantors.length;
        for (uint256 i = 0; i < activationEnd; ++i) {
            _execute(timelock, calls[i].target, calls[i].data, calls[i].salt, genesisStartNonce + i);
        }
        emit BetaGovernancePhase("GENESIS_ACTIVATED", addresses.blueprint, genesisStartNonce, activationEnd);
        vm.stopBroadcast();
    }

    function finalize(
        Addresses calldata addresses,
        PaxeerBetaDeploymentValidator.Input calldata input,
        PaxeerBetaDeploymentValidator.GuarantorInput[] calldata guarantors,
        uint256 genesisStartNonce
    ) external {
        _requireBootstrap(addresses, input);
        GuarantorBond bond = GuarantorBond(payable(addresses.guarantorBond));
        for (uint256 i = 0; i < guarantors.length; ++i) {
            if (bond.bondRecord(guarantors[i].guarantorId).amount != guarantors[i].bondAmount) {
                revert InvalidDeploymentState();
            }
        }
        uint256 key = vm.envUint("EVM_WALLET_PRIVATE_KEY");
        vm.startBroadcast(key);
        ScheduledCall[] memory calls = _genesisCalls(addresses, input, guarantors);
        LayerXTimelock timelock = LayerXTimelock(payable(addresses.timelock));
        uint256 start = 4 + guarantors.length;
        for (uint256 i = start; i < calls.length; ++i) {
            _execute(timelock, calls[i].target, calls[i].data, calls[i].salt, genesisStartNonce + i);
        }
        _requireFinal(addresses, input, guarantors);
        Blueprint(addresses.blueprint).seal();
        if (!Blueprint(addresses.blueprint).deploymentsSealed()) revert InvalidDeploymentState();
        bytes32 deploymentId = ManagerContainer(addresses.managerContainer).deploymentId();
        emit BetaDeploymentComplete(deploymentId, addresses.blueprint, addresses);
        vm.stopBroadcast();
    }

    function depositGenesisBond(address guarantorBond, bytes32 guarantorId, uint256 amount) external {
        if (block.chainid != 125 || guarantorBond.code.length == 0 || guarantorId == bytes32(0) || amount == 0) {
            revert InvalidDeploymentState();
        }
        GuarantorBond bond = GuarantorBond(payable(guarantorBond));
        GuarantorBond.BondRecord memory beforeRecord = bond.bondRecord(guarantorId);
        uint256 key = vm.envUint("EVM_WALLET_PRIVATE_KEY");
        address controller = vm.addr(key);
        if (beforeRecord.bondController != controller || beforeRecord.amount != 0) {
            revert InvalidDeploymentState();
        }
        vm.startBroadcast(key);
        bond.depositBond(guarantorId, amount);
        vm.stopBroadcast();
        if (bond.bondRecord(guarantorId).amount != amount) revert InvalidDeploymentState();
        emit BetaBondFunded(guarantorId, controller, amount);
    }

    function _deployTimelock(Blueprint blueprint, PaxeerBetaDeploymentValidator.Input calldata input, uint192 release)
        private
        returns (LayerXTimelock deployed)
    {
        bytes memory arguments = abi.encode(
            input.timelockDelay,
            input.timelockGracePeriod,
            input.bootstrapOperator,
            input.bootstrapOperator,
            input.bootstrapOperator,
            input.timelockMaximumCallValue,
            blueprint.staticConfigHash(),
            release
        );
        vm.stopBroadcast();
        LayerXTimelock runtimeReference = new LayerXTimelock(
            input.timelockDelay,
            input.timelockGracePeriod,
            input.bootstrapOperator,
            input.bootstrapOperator,
            input.bootstrapOperator,
            input.timelockMaximumCallValue,
            blueprint.staticConfigHash(),
            release
        );
        vm.startBroadcast(broadcastKey);
        deployed = LayerXTimelock(
            payable(blueprint.deployTimelock(
                    abi.encodePacked(type(LayerXTimelock).creationCode, arguments), address(runtimeReference).codehash
                ))
        );
    }

    function _deploy(Blueprint blueprint, bytes32 role, bytes memory code, bytes32 runtimeHash)
        private
        returns (address result)
    {
        result = blueprint.deploy(role, code, runtimeHash);
        if (result != blueprint.predict(role, keccak256(code))) revert InvalidDeploymentState();
    }

    function _assetRegistry(
        Blueprint b,
        Addresses memory,
        PaxeerBetaDeploymentValidator.Input calldata input,
        bytes32 hash,
        uint192 release
    ) private returns (address) {
        bytes memory args = abi.encode(b.governanceTimelock(), input.emergencyCouncil, hash, release);
        vm.stopBroadcast();
        AssetRegistry ref = new AssetRegistry(b.governanceTimelock(), input.emergencyCouncil, hash, release);
        vm.startBroadcast(broadcastKey);
        return _deploy(
            b,
            Predeploys.ASSET_REGISTRY,
            abi.encodePacked(type(AssetRegistry).creationCode, args),
            address(ref).codehash
        );
    }

    function _vault(
        Blueprint b,
        Addresses memory a,
        PaxeerBetaDeploymentValidator.Input calldata input,
        bytes32 hash,
        uint192 release
    ) private returns (address) {
        bytes memory args =
            abi.encode(AssetRegistry(a.assetRegistry), a.timelock, input.emergencyCouncil, hash, release);
        vm.stopBroadcast();
        LayerXVault ref =
            new LayerXVault(AssetRegistry(a.assetRegistry), a.timelock, input.emergencyCouncil, hash, release);
        vm.startBroadcast(broadcastKey);
        return
            _deploy(b, Predeploys.VAULT, abi.encodePacked(type(LayerXVault).creationCode, args), address(ref).codehash);
    }

    function _bond(
        Blueprint b,
        Addresses memory a,
        PaxeerBetaDeploymentValidator.Input calldata input,
        uint32 networkId,
        bytes32 hash,
        uint192 release,
        uint16 selectedProtocolVersion
    ) private returns (address) {
        bytes memory args = abi.encode(
            a.timelock,
            a.timelock,
            Constants.USDL_TOKEN,
            a.vault,
            Constants.USDL_ASSET_ID,
            selectedProtocolVersion,
            networkId,
            input.minimumBondBps,
            input.unbondingDelay,
            hash,
            release
        );
        vm.stopBroadcast();
        GuarantorBond ref = new GuarantorBond(
            a.timelock,
            a.timelock,
            Constants.USDL_TOKEN,
            a.vault,
            Constants.USDL_ASSET_ID,
            selectedProtocolVersion,
            networkId,
            input.minimumBondBps,
            input.unbondingDelay,
            hash,
            release
        );
        vm.startBroadcast(broadcastKey);
        return _deploy(
            b,
            Predeploys.GUARANTOR_BOND,
            abi.encodePacked(type(GuarantorBond).creationCode, args),
            address(ref).codehash
        );
    }

    function _checkpoint(
        Blueprint b,
        Addresses memory a,
        PaxeerBetaDeploymentValidator.Input calldata input,
        PaxeerBetaDeploymentValidator.GenesisArtifacts memory g,
        bytes32 hash,
        uint192 release,
        uint16 selectedProtocolVersion
    ) private returns (address) {
        bytes memory args = abi.encode(
            GuarantorBond(payable(a.guarantorBond)),
            selectedProtocolVersion,
            g.networkId,
            input.checkpointThresholdNumerator,
            input.checkpointThresholdDenominator,
            input.checkpointMaximumAge,
            input.checkpointFutureDrift,
            g.manifestDigest,
            g.canonicalStateRoot,
            g.receiptRoot,
            hash,
            release
        );
        vm.stopBroadcast();
        CheckpointRegistry ref = new CheckpointRegistry(
            GuarantorBond(payable(a.guarantorBond)),
            selectedProtocolVersion,
            g.networkId,
            input.checkpointThresholdNumerator,
            input.checkpointThresholdDenominator,
            input.checkpointMaximumAge,
            input.checkpointFutureDrift,
            g.manifestDigest,
            g.canonicalStateRoot,
            g.receiptRoot,
            hash,
            release
        );
        vm.startBroadcast(broadcastKey);
        return _deploy(
            b,
            Predeploys.CHECKPOINT_REGISTRY,
            abi.encodePacked(type(CheckpointRegistry).creationCode, args),
            address(ref).codehash
        );
    }

    function _challenge(
        Blueprint b,
        Addresses memory a,
        PaxeerBetaDeploymentValidator.Input calldata input,
        bytes32 hash,
        uint192 release
    ) private returns (address) {
        bytes memory args = abi.encode(
            CheckpointRegistry(a.checkpointRegistry),
            GuarantorBond(payable(a.guarantorBond)),
            a.timelock,
            input.emergencyCouncil,
            input.challengeWindow,
            input.challengeBond,
            hash,
            release
        );
        vm.stopBroadcast();
        CheckpointChallengeManager ref = new CheckpointChallengeManager(
            CheckpointRegistry(a.checkpointRegistry),
            GuarantorBond(payable(a.guarantorBond)),
            a.timelock,
            input.emergencyCouncil,
            input.challengeWindow,
            input.challengeBond,
            hash,
            release
        );
        vm.startBroadcast(broadcastKey);
        return _deploy(
            b,
            Predeploys.CHALLENGE_MANAGER,
            abi.encodePacked(type(CheckpointChallengeManager).creationCode, args),
            address(ref).codehash
        );
    }

    function _nullifier(Blueprint b, PaxeerBetaDeploymentValidator.Input calldata input, bytes32 hash, uint192 release)
        private
        returns (address)
    {
        bytes memory args = abi.encode(b.governanceTimelock(), input.emergencyCouncil, hash, release);
        vm.stopBroadcast();
        WithdrawalNullifierRegistry ref =
            new WithdrawalNullifierRegistry(b.governanceTimelock(), input.emergencyCouncil, hash, release);
        vm.startBroadcast(broadcastKey);
        return _deploy(
            b,
            Predeploys.NULLIFIER_REGISTRY,
            abi.encodePacked(type(WithdrawalNullifierRegistry).creationCode, args),
            address(ref).codehash
        );
    }

    function _claims(Blueprint b, Addresses memory a, bytes32 hash, uint192 release) private returns (address) {
        bytes memory args = abi.encode(
            CheckpointRegistry(a.checkpointRegistry),
            CheckpointChallengeManager(payable(a.challengeManager)),
            WithdrawalNullifierRegistry(a.nullifierRegistry),
            LayerXVault(a.vault),
            hash,
            release
        );
        vm.stopBroadcast();
        WithdrawalClaims ref = new WithdrawalClaims(
            CheckpointRegistry(a.checkpointRegistry),
            CheckpointChallengeManager(payable(a.challengeManager)),
            WithdrawalNullifierRegistry(a.nullifierRegistry),
            LayerXVault(a.vault),
            hash,
            release
        );
        vm.startBroadcast(broadcastKey);
        return _deploy(
            b,
            Predeploys.WITHDRAWAL_CLAIMS,
            abi.encodePacked(type(WithdrawalClaims).creationCode, args),
            address(ref).codehash
        );
    }

    function _exit(
        Blueprint b,
        Addresses memory a,
        PaxeerBetaDeploymentValidator.Input calldata input,
        bytes32 hash,
        uint192 release
    ) private returns (address) {
        bytes memory args = abi.encode(
            CheckpointRegistry(a.checkpointRegistry),
            CheckpointChallengeManager(payable(a.challengeManager)),
            WithdrawalNullifierRegistry(a.nullifierRegistry),
            LayerXVault(a.vault),
            a.timelock,
            input.emergencyCouncil,
            input.emergencyDelay,
            hash,
            release
        );
        vm.stopBroadcast();
        EmergencyExit ref = new EmergencyExit(
            CheckpointRegistry(a.checkpointRegistry),
            CheckpointChallengeManager(payable(a.challengeManager)),
            WithdrawalNullifierRegistry(a.nullifierRegistry),
            LayerXVault(a.vault),
            a.timelock,
            input.emergencyCouncil,
            input.emergencyDelay,
            hash,
            release
        );
        vm.startBroadcast(broadcastKey);
        return _deploy(
            b,
            Predeploys.EMERGENCY_EXIT,
            abi.encodePacked(type(EmergencyExit).creationCode, args),
            address(ref).codehash
        );
    }

    function _reconciler(Blueprint b, Addresses memory a, bytes32 hash, uint192 release) private returns (address) {
        bytes memory args = abi.encode(
            CheckpointRegistry(a.checkpointRegistry),
            LayerXVault(a.vault),
            WithdrawalClaims(a.withdrawalClaims),
            hash,
            release
        );
        vm.stopBroadcast();
        ReserveReconciler ref = new ReserveReconciler(
            CheckpointRegistry(a.checkpointRegistry),
            LayerXVault(a.vault),
            WithdrawalClaims(a.withdrawalClaims),
            hash,
            release
        );
        vm.startBroadcast(broadcastKey);
        return _deploy(
            b,
            Predeploys.RESERVE_RECONCILER,
            abi.encodePacked(type(ReserveReconciler).creationCode, args),
            address(ref).codehash
        );
    }

    function _manager(Blueprint b, StaticConfig.Config memory config, uint192) private returns (address) {
        bytes memory args = abi.encode(config);
        vm.stopBroadcast();
        ManagerContainer ref = new ManagerContainer(config);
        vm.startBroadcast(broadcastKey);
        return _deploy(
            b,
            Predeploys.CONTRACTS_MANAGER,
            abi.encodePacked(type(ManagerContainer).creationCode, args),
            address(ref).codehash
        );
    }

    function _migrator(Blueprint b, Addresses memory a, PaxeerBetaDeploymentValidator.Input calldata input, uint192)
        private
        returns (address)
    {
        bytes memory args = abi.encode(
            ManagerContainer(a.managerContainer),
            a.timelock,
            input.bootstrapOperator,
            input.migrationDelay,
            input.migrationExpiry,
            input.migrationGasLimit,
            input.migrationMaximumCallValue
        );
        vm.stopBroadcast();
        ManagerMigrator ref = new ManagerMigrator(
            ManagerContainer(a.managerContainer),
            a.timelock,
            input.bootstrapOperator,
            input.migrationDelay,
            input.migrationExpiry,
            input.migrationGasLimit,
            input.migrationMaximumCallValue
        );
        vm.startBroadcast(broadcastKey);
        return _deploy(
            b,
            Predeploys.MANAGER_MIGRATOR,
            abi.encodePacked(type(ManagerMigrator).creationCode, args),
            address(ref).codehash
        );
    }

    function _topology(Blueprint b, Addresses memory a, bytes32 hash, uint192 release) private returns (address) {
        bytes memory args = abi.encode(
            CheckpointRegistry(a.checkpointRegistry),
            GuarantorBond(payable(a.guarantorBond)),
            LayerXVault(a.vault),
            CheckpointChallengeManager(payable(a.challengeManager)),
            WithdrawalNullifierRegistry(a.nullifierRegistry),
            WithdrawalClaims(a.withdrawalClaims),
            hash,
            release
        );
        vm.stopBroadcast();
        LayerXCustody ref = new LayerXCustody(
            CheckpointRegistry(a.checkpointRegistry),
            GuarantorBond(payable(a.guarantorBond)),
            LayerXVault(a.vault),
            CheckpointChallengeManager(payable(a.challengeManager)),
            WithdrawalNullifierRegistry(a.nullifierRegistry),
            WithdrawalClaims(a.withdrawalClaims),
            hash,
            release
        );
        vm.startBroadcast(broadcastKey);
        return _deploy(
            b,
            Predeploys.CUSTODY_TOPOLOGY,
            abi.encodePacked(type(LayerXCustody).creationCode, args),
            address(ref).codehash
        );
    }

    function _permissions(Addresses memory a)
        private
        pure
        returns (address[] memory targets, bytes4[] memory selectors)
    {
        targets = new address[](21);
        selectors = new bytes4[](21);
        uint256 i;
        targets[i] = a.assetRegistry;
        selectors[i++] = AssetRegistry.registerAsset.selector;
        targets[i] = a.assetRegistry;
        selectors[i++] = AssetRegistry.updateRisk.selector;
        targets[i] = a.assetRegistry;
        selectors[i++] = AssetRegistry.governanceUnpause.selector;
        targets[i] = a.vault;
        selectors[i++] = LayerXVault.setSettlementModule.selector;
        targets[i] = a.vault;
        selectors[i++] = LayerXVault.setGuarantorBond.selector;
        targets[i] = a.guarantorBond;
        selectors[i++] = GuarantorBond.setSlashingAuthority.selector;
        targets[i] = a.guarantorBond;
        selectors[i++] = GuarantorBond.activateGuarantor.selector;
        targets[i] = a.guarantorBond;
        selectors[i++] = GuarantorBond.rotateGuarantorSigner.selector;
        targets[i] = a.guarantorBond;
        selectors[i++] = GuarantorBond.removeGuarantor.selector;
        targets[i] = a.guarantorBond;
        selectors[i++] = GuarantorBond.setGuarantorJailStatus.selector;
        targets[i] = a.guarantorBond;
        selectors[i++] = GuarantorBond.setUnresolvedSlashing.selector;
        targets[i] = a.guarantorBond;
        selectors[i++] = GuarantorBond.sweepSlashed.selector;
        targets[i] = a.guarantorBond;
        selectors[i++] = GuarantorBond.sealGenesisBondedSet.selector;
        targets[i] = a.challengeManager;
        selectors[i++] = CheckpointChallengeManager.resolveChallenge.selector;
        targets[i] = a.nullifierRegistry;
        selectors[i++] = WithdrawalNullifierRegistry.setConsumer.selector;
        targets[i] = a.emergencyExit;
        selectors[i++] = EmergencyExit.declareEmergency.selector;
        targets[i] = a.managerContainer;
        selectors[i++] = ManagerContainer.initialize.selector;
        targets[i] = a.managerContainer;
        selectors[i++] = ManagerContainer.setMigrator.selector;
        targets[i] = a.managerContainer;
        selectors[i++] = ManagerContainer.finalizeGenesis.selector;
        targets[i] = a.managerMigrator;
        selectors[i++] = ManagerMigrator.stageMigration.selector;
        targets[i] = a.managerMigrator;
        selectors[i++] = ManagerMigrator.cancelMigration.selector;
        if (i != 21) revert InvalidDeploymentState();
    }

    function _schedulePermissions(Addresses memory a, uint64 delay) private {
        (address[] memory targets, bytes4[] memory selectors) = _permissions(a);
        LayerXTimelock timelock = LayerXTimelock(payable(a.timelock));
        for (uint256 i = 0; i < targets.length; ++i) {
            bytes memory data = abi.encodeCall(LayerXTimelock.setCallPermission, (targets[i], selectors[i], true));
            timelock.schedule(a.timelock, 0, data, _salt("PERMISSION", i, a.timelock, data), delay);
        }
    }

    function _scheduleGenesis(
        Addresses memory a,
        PaxeerBetaDeploymentValidator.Input calldata input,
        PaxeerBetaDeploymentValidator.GuarantorInput[] calldata guarantors
    ) private {
        ScheduledCall[] memory calls = _genesisCalls(a, input, guarantors);
        LayerXTimelock timelock = LayerXTimelock(payable(a.timelock));
        for (uint256 i = 0; i < calls.length; ++i) {
            timelock.schedule(calls[i].target, 0, calls[i].data, calls[i].salt, input.timelockDelay);
        }
    }

    function _genesisCalls(
        Addresses memory a,
        PaxeerBetaDeploymentValidator.Input calldata input,
        PaxeerBetaDeploymentValidator.GuarantorInput[] calldata guarantors
    ) private view returns (ScheduledCall[] memory calls) {
        calls = new ScheduledCall[](12 + guarantors.length);
        uint256 i;
        calls[i] = _call(
            "GENESIS",
            i,
            a.managerContainer,
            abi.encodeCall(ManagerContainer.initialize, (_manifests(a), _allowlists()))
        );
        ++i;
        calls[i] =
            _call("GENESIS", i, a.managerContainer, abi.encodeCall(ManagerContainer.setMigrator, (a.managerMigrator)));
        ++i;
        calls[i] = _call(
            "GENESIS",
            i,
            a.assetRegistry,
            abi.encodeCall(
                AssetRegistry.registerAsset,
                (
                    Constants.USDL_ASSET_ID,
                    Constants.USDL_TOKEN,
                    Constants.USDL_TOKEN_DECIMALS,
                    input.usdlMinimumDeposit,
                    input.usdlCustodyCap
                )
            )
        );
        ++i;
        calls[i] = _call("GENESIS", i, a.vault, abi.encodeCall(LayerXVault.setGuarantorBond, (a.guarantorBond)));
        ++i;
        for (uint256 j = 0; j < guarantors.length; ++j) {
            PaxeerBetaDeploymentValidator.GuarantorInput calldata g = guarantors[j];
            calls[i] = _call(
                "GENESIS",
                i,
                a.guarantorBond,
                abi.encodeCall(
                    GuarantorBond.activateGuarantor,
                    (g.guarantorId, g.signer, g.bondController, g.joinedEpoch, g.governanceSequence)
                )
            );
            ++i;
        }
        bytes32[] memory ids = new bytes32[](guarantors.length);
        for (uint256 j = 0; j < guarantors.length; ++j) {
            ids[j] = guarantors[j].guarantorId;
        }
        calls[i] = _call("GENESIS", i, a.guarantorBond, abi.encodeCall(GuarantorBond.sealGenesisBondedSet, (ids)));
        ++i;
        calls[i] = _call("GENESIS", i, a.managerContainer, abi.encodeCall(ManagerContainer.finalizeGenesis, ()));
        ++i;
        calls[i] = _call(
            "GENESIS", i, a.timelock, abi.encodeCall(LayerXTimelock.setRole, (uint8(1), input.finalProposer, true))
        );
        ++i;
        calls[i] = _call(
            "GENESIS", i, a.timelock, abi.encodeCall(LayerXTimelock.setRole, (uint8(2), input.finalExecutor, true))
        );
        ++i;
        calls[i] = _call(
            "GENESIS", i, a.timelock, abi.encodeCall(LayerXTimelock.setRole, (uint8(3), input.emergencyCouncil, true))
        );
        ++i;
        calls[i] = _call(
            "GENESIS", i, a.timelock, abi.encodeCall(LayerXTimelock.setRole, (uint8(1), input.bootstrapOperator, false))
        );
        ++i;
        calls[i] = _call(
            "GENESIS", i, a.timelock, abi.encodeCall(LayerXTimelock.setRole, (uint8(3), input.bootstrapOperator, false))
        );
        ++i;
        calls[i] = _call(
            "GENESIS", i, a.timelock, abi.encodeCall(LayerXTimelock.setRole, (uint8(2), input.bootstrapOperator, false))
        );
        ++i;
        if (i != calls.length) revert InvalidDeploymentState();
    }

    function _call(bytes32 phase, uint256 index, address target, bytes memory data)
        private
        pure
        returns (ScheduledCall memory)
    {
        return ScheduledCall({target: target, data: data, salt: _salt(phase, index, target, data), nonce: 0});
    }

    function _salt(bytes32 phase, uint256 index, address target, bytes memory data) private pure returns (bytes32) {
        return keccak256(abi.encode("LXP/Paxeer/beta-deployment/v1", phase, index, target, keccak256(data)));
    }

    function _execute(LayerXTimelock timelock, address target, bytes memory data, bytes32 salt, uint256 nonce) private {
        timelock.execute(target, 0, data, salt, nonce);
    }

    function _allowlists() private pure returns (bytes4[][] memory lists) {
        lists = new bytes4[][](Predeploys.COUNT);
        for (uint256 i = 0; i < lists.length; ++i) {
            lists[i] = new bytes4[](0);
        }
    }

    function _manifests(Addresses memory a) private view returns (Preinstalls.ComponentManifest[] memory manifests) {
        manifests = new Preinstalls.ComponentManifest[](Predeploys.COUNT);
        for (uint256 i = 0; i < manifests.length; ++i) {
            bytes32 role = Predeploys.roleAt(i);
            address component = _component(a, role);
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

    function _component(Addresses memory a, bytes32 role) private pure returns (address) {
        if (role == Predeploys.TIMELOCK) return a.timelock;
        if (role == Predeploys.ASSET_REGISTRY) return a.assetRegistry;
        if (role == Predeploys.VAULT) return a.vault;
        if (role == Predeploys.GUARANTOR_BOND) return a.guarantorBond;
        if (role == Predeploys.CHECKPOINT_REGISTRY) return a.checkpointRegistry;
        if (role == Predeploys.CHALLENGE_MANAGER) return a.challengeManager;
        if (role == Predeploys.NULLIFIER_REGISTRY) return a.nullifierRegistry;
        if (role == Predeploys.WITHDRAWAL_CLAIMS) return a.withdrawalClaims;
        if (role == Predeploys.EMERGENCY_EXIT) return a.emergencyExit;
        if (role == Predeploys.RESERVE_RECONCILER) return a.reserveReconciler;
        if (role == Predeploys.CONTRACTS_MANAGER) return a.managerContainer;
        if (role == Predeploys.MANAGER_MIGRATOR) return a.managerMigrator;
        if (role == Predeploys.CUSTODY_TOPOLOGY) return a.custodyTopology;
        revert InvalidDeploymentState();
    }

    function _requireBootstrap(Addresses memory a, PaxeerBetaDeploymentValidator.Input calldata input) private view {
        if (
            block.chainid != 125 || a.blueprint.code.length == 0
                || Blueprint(a.blueprint).manager() != input.bootstrapOperator
                || Blueprint(a.blueprint).governanceTimelock() != a.timelock
                || Blueprint(a.blueprint).deploymentsSealed()
                || (GuarantorBond(payable(a.guarantorBond)).protocolVersion() != 2
                    && GuarantorBond(payable(a.guarantorBond)).protocolVersion() != 3)
                || GuarantorBond(payable(a.guarantorBond)).protocolVersion()
                    != CheckpointRegistry(a.checkpointRegistry).protocolVersion()
        ) revert InvalidDeploymentState();
        for (uint256 i = 0; i < Predeploys.COUNT; ++i) {
            if (Blueprint(a.blueprint).deploymentForRole(Predeploys.roleAt(i)) != _component(a, Predeploys.roleAt(i))) {
                revert InvalidDeploymentState();
            }
        }
    }

    function _requireFinal(
        Addresses memory a,
        PaxeerBetaDeploymentValidator.Input calldata input,
        PaxeerBetaDeploymentValidator.GuarantorInput[] calldata guarantors
    ) private view {
        LayerXTimelock timelock = LayerXTimelock(payable(a.timelock));
        ManagerContainer manager = ManagerContainer(a.managerContainer);
        GuarantorBond bond = GuarantorBond(payable(a.guarantorBond));
        if (
            !timelock.proposer(input.finalProposer) || !timelock.executor(input.finalExecutor)
                || !timelock.guardian(input.emergencyCouncil) || timelock.proposer(input.bootstrapOperator)
                || timelock.executor(input.bootstrapOperator) || timelock.guardian(input.bootstrapOperator)
                || !manager.initialized() || !manager.genesisFinalized() || manager.deploymentId() == bytes32(0)
                || manager.migrator() != a.managerMigrator || bond.genesisBondedSetCommitment() == bytes32(0)
                || bond.genesisBondedSetVersion() != bond.membershipVersion()
        ) revert InvalidDeploymentState();
        for (uint256 i = 0; i < Predeploys.COUNT; ++i) {
            bytes32 role = Predeploys.roleAt(i);
            if (manager.componentForRole(role) != _component(a, role)) revert InvalidDeploymentState();
        }
        for (uint256 i = 0; i < guarantors.length; ++i) {
            GuarantorBond.BondRecord memory record = bond.bondRecord(guarantors[i].guarantorId);
            if (
                record.signer != guarantors[i].signer || record.bondController != guarantors[i].bondController
                    || record.amount != guarantors[i].bondAmount || record.ejectedAtVersion != 0
            ) {
                revert InvalidDeploymentState();
            }
        }
    }
}
