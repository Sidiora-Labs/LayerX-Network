// SPDX-License-Identifier: LicenseRef-Centra-ai-Protocol
pragma solidity ^0.8.24;

import {ILayerXComponent, Preinstalls} from "../contracts/deployment/Preinstalls.sol";
import {Predeploys} from "../contracts/deployment/Predeploys.sol";
import {Blueprint} from "../contracts/deployment/Blueprint.sol";
import {StaticConfig} from "../contracts/config/StaticConfig.sol";
import {LayerXTimelock} from "../contracts/governance/LayerXTimelock.sol";
import {Constants} from "../contracts/libraries/Constants.sol";
import {SemverComp} from "../contracts/libraries/SemverComp.sol";
import {ManagerContainer} from "../contracts/manager/ManagerContainer.sol";
import {ManagerMigrator} from "../contracts/manager/ManagerMigrator.sol";
import {StandardValidatorUtils} from "../contracts/manager/StandardValidatorUtils.sol";
import {
    ManagerUnauthorized,
    InvalidMigrationState,
    InvalidMigrationWindow,
    InvalidMigrationVersion,
    MigrationCommitmentMismatch,
    InvalidMigrationOperation,
    MigrationCodeHashMismatch,
    MigrationStorageLayoutMismatch,
    MigrationCallFailed,
    SelectorNotAllowed,
    InvalidComponent
} from "../contracts/manager/BlockErrors.sol";

interface ManagerVm {
    function deal(address account, uint256 balance) external;
    function expectPartialRevert(bytes4 selector) external;
    function expectRevert() external;
    function prank(address sender) external;
    function warp(uint256 timestamp) external;
}

contract MigrationComponent is ILayerXComponent {
    error ComponentUnauthorized();
    error RequestedMigrationFailure();
    error InvalidLayoutTransition();

    bytes32 public immutable override componentRole;
    bytes32 public immutable override staticConfigHash;
    uint192 public immutable override releaseVersion;
    address public immutable governance;
    address public immutable emergencyCouncil;
    address public immutable custodyAuthority;
    address public immutable membershipAuthority;
    address public immutable governanceTimelock;
    uint16 public override storageLayoutVersion;
    address public migrator;
    address public container;
    address public assetRegistry;
    address public guarantorEligibility;
    address public registry;
    address public guarantorBond;
    address public challengeManager;
    address public nullifierRegistry;
    address public vault;
    address public withdrawalClaims;
    address public checkpointRegistry;
    address public withdrawalNullifiers;
    uint256 public migrationCount;
    uint256 public receivedValue;

    function bondToken() external pure returns (address) {
        return Constants.USDL_TOKEN;
    }

    function assetId() external pure returns (bytes32) {
        return Constants.USDL_ASSET_ID;
    }

    constructor(
        bytes32 role,
        bytes32 configHash,
        uint192 release,
        address governanceAuthority,
        address emergencyAuthority
    ) {
        componentRole = role;
        staticConfigHash = configHash;
        releaseVersion = release;
        address authority = governanceAuthority == address(0) ? address(this) : governanceAuthority;
        governance = authority;
        emergencyCouncil = emergencyAuthority;
        custodyAuthority = authority;
        membershipAuthority = authority;
        governanceTimelock = authority;
        storageLayoutVersion = Constants.STORAGE_LAYOUT_VERSION;
    }

    function configureTopology(address[] calldata topology) external {
        if (msg.sender != governance || topology.length != 9 || container != address(0)) {
            revert ComponentUnauthorized();
        }
        assetRegistry = topology[0];
        vault = topology[1];
        guarantorEligibility = topology[2];
        guarantorBond = topology[2];
        registry = topology[3];
        checkpointRegistry = topology[3];
        challengeManager = topology[4];
        nullifierRegistry = topology[5];
        withdrawalNullifiers = topology[5];
        withdrawalClaims = topology[6];
        container = topology[7];
        if (topology[8] == address(0)) revert ComponentUnauthorized();
    }

    function authorizeMigrator(address migrationExecutor) external {
        if (msg.sender != governance || migrator != address(0) || migrationExecutor.code.length == 0) {
            revert ComponentUnauthorized();
        }
        migrator = migrationExecutor;
    }

    function migrate(uint16 nextLayout, bool fail) external payable {
        if (msg.sender != migrator) revert ComponentUnauthorized();
        if (fail) revert RequestedMigrationFailure();
        if (nextLayout < storageLayoutVersion || nextLayout > storageLayoutVersion + 1) {
            revert InvalidLayoutTransition();
        }
        storageLayoutVersion = nextLayout;
        ++migrationCount;
        receivedValue += msg.value;
    }
}

    contract NonCanonicalAddressComponent is ILayerXComponent {
        bytes32 public constant override componentRole = Predeploys.ASSET_REGISTRY;
        uint16 public constant override storageLayoutVersion = Constants.STORAGE_LAYOUT_VERSION;
        bytes32 public immutable override staticConfigHash;
        uint192 public immutable override releaseVersion;
        address public immutable emergencyCouncil;
        uint256 private malformedGovernance;

        constructor(bytes32 configHash, uint192 release, address expectedGovernance, address emergencyAuthority) {
            staticConfigHash = configHash;
            releaseVersion = release;
            emergencyCouncil = emergencyAuthority;
            malformedGovernance = uint256(uint160(expectedGovernance)) | (uint256(1) << 200);
        }

        function governance() external view returns (address) {
            uint256 word = malformedGovernance;
            assembly ("memory-safe") {
                mstore(0, word)
                return(0, 32)
            }
        }
    }

        contract ContractManagerTest {
            ManagerVm private constant vm = ManagerVm(address(uint160(uint256(keccak256("hevm cheat code")))));

            address private constant EMERGENCY_COUNCIL = address(0xEC01);
            ManagerContainer private container;
            ManagerMigrator private migrator;
            MigrationComponent[] private components;
            address[] private manifestComponents;
            address private governanceTimelock;
            bytes32 private configHash;
            StaticConfig.Config private managerConfig;
            uint192 private release100;
            uint192 private release101;
            uint192 private release110;

            function setUp() public {
                release100 = SemverComp.parseRelease("1.0.0");
                release101 = SemverComp.parseRelease("1.0.1");
                release110 = SemverComp.parseRelease("1.1.0");
                StaticConfig.Config memory config = _config(address(0), EMERGENCY_COUNCIL, release100);
                Blueprint blueprint = new Blueprint(address(this), config);
                governanceTimelock = blueprint.predictTimelock();
                config.governanceTimelock = governanceTimelock;
                configHash = StaticConfig.hash(config, block.chainid);
                managerConfig = config;
                bytes memory timelockArguments = abi.encode(
                    uint64(2 days),
                    uint64(7 days),
                    address(this),
                    address(this),
                    address(this),
                    uint256(1 ether),
                    configHash,
                    release100
                );
                LayerXTimelock runtimeReference = new LayerXTimelock(
                    2 days, 7 days, address(this), address(this), address(this), 1 ether, configHash, release100
                );
                require(
                    blueprint.deployTimelock(
                        abi.encodePacked(type(LayerXTimelock).creationCode, timelockArguments),
                        address(runtimeReference).codehash
                    ) == governanceTimelock,
                    "timelock"
                );
                MigrationComponent timelockComponent =
                    new MigrationComponent(
                    Predeploys.TIMELOCK, configHash, release100, governanceTimelock, EMERGENCY_COUNCIL
                );
                container = new ManagerContainer(config);
                migrator =
                    new ManagerMigrator(
                    container, governanceTimelock, address(this), 1 days, 7 days, 1_000_000, 1 ether
                );

                Preinstalls.ComponentManifest[] memory manifests = new Preinstalls.ComponentManifest[](Predeploys.COUNT);
                bytes4[][] memory allowlists = new bytes4[][](Predeploys.COUNT);
                for (uint256 i = 0; i < Predeploys.COUNT; ++i) {
                    bytes32 role = Predeploys.roleAt(i);
                    MigrationComponent component = i == 0
                        ? timelockComponent
                        : new MigrationComponent(role, configHash, release100, governanceTimelock, EMERGENCY_COUNCIL);
                    if (i != 0) components.push(component);
                    address manifestComponent = role == Predeploys.TIMELOCK ? governanceTimelock : address(component);
                    if (role == Predeploys.CONTRACTS_MANAGER) manifestComponent = address(container);
                    else if (role == Predeploys.MANAGER_MIGRATOR) manifestComponent = address(migrator);
                    manifestComponents.push(manifestComponent);
                    ILayerXComponent attested = ILayerXComponent(manifestComponent);
                    manifests[i] = Preinstalls.ComponentManifest({
                        role: role,
                        component: manifestComponent,
                        interfaceId: Preinstalls.interfaceId(),
                        runtimeCodeHash: manifestComponent.codehash,
                        configHash: attested.staticConfigHash(),
                        release: attested.releaseVersion(),
                        storageLayout: attested.storageLayoutVersion()
                    });
                    allowlists[i] = new bytes4[](1);
                    allowlists[i][0] = MigrationComponent.migrate.selector;
                }
                address[] memory topology = new address[](9);
                topology[0] = address(components[0]);
                topology[1] = address(components[1]);
                topology[2] = address(components[2]);
                topology[3] = address(components[3]);
                topology[4] = address(components[4]);
                topology[5] = address(components[5]);
                topology[6] = address(components[6]);
                topology[7] = address(container);
                topology[8] = address(migrator);
                for (uint256 i = 0; i < components.length; ++i) {
                    vm.prank(governanceTimelock);
                    components[i].configureTopology(topology);
                }
                vm.prank(governanceTimelock);
                container.initialize(manifests, allowlists);
                vm.prank(governanceTimelock);
                container.setMigrator(address(migrator));
                for (uint256 i = 0; i < components.length; ++i) {
                    vm.prank(governanceTimelock);
                    components[i].authorizeMigrator(address(migrator));
                }
            }

            function testExecutesCommittedMigrationExactlyOnce() public {
                StandardValidatorUtils.Operation[] memory operations = _operations(1, false);
                bytes32 migrationId = _stage(release101, operations, 1 days, 2 days);
                vm.warp(block.timestamp + 1 days);
                migrator.executeMigration(migrationId, operations);
                require(components[0].storageLayoutVersion() == 2, "layout");
                require(components[0].migrationCount() == 1, "count");
                require(container.currentRelease() == release101, "release");
                require(migrator.activeMigration() == bytes32(0), "active");
                vm.expectPartialRevert(InvalidMigrationState.selector);
                migrator.executeMigration(migrationId, operations);
            }

            function testMigrationCannotExecuteEarlyOrAfterExpiry() public {
                StandardValidatorUtils.Operation[] memory operations = _operations(1, false);
                bytes32 migrationId = _stage(release101, operations, 1 days, 2 days);
                vm.expectPartialRevert(InvalidMigrationWindow.selector);
                migrator.executeMigration(migrationId, operations);
                vm.warp(block.timestamp + 3 days + 1);
                vm.expectPartialRevert(InvalidMigrationWindow.selector);
                migrator.executeMigration(migrationId, operations);
            }

            function testRejectsVersionDowngradeAndConcurrentPlan() public {
                StandardValidatorUtils.Operation[] memory operations = _operations(1, false);
                vm.expectPartialRevert(InvalidMigrationVersion.selector);
                _stage(release100, operations, 1 days, 2 days);
                bytes32 migrationId = _stage(release101, operations, 1 days, 2 days);
                require(migrationId != bytes32(0), "stage");
                vm.expectPartialRevert(InvalidMigrationState.selector);
                _stage(release110, operations, 1 days, 2 days);
            }

            function testOrderedOperationRootRejectsReordering() public {
                StandardValidatorUtils.Operation[] memory operations = _operations(2, false);
                bytes32 migrationId = _stage(release101, operations, 1 days, 2 days);
                StandardValidatorUtils.Operation memory temporary = operations[0];
                operations[0] = operations[1];
                operations[1] = temporary;
                vm.warp(block.timestamp + 1 days);
                vm.expectPartialRevert(MigrationCommitmentMismatch.selector);
                migrator.executeMigration(migrationId, operations);
                require(
                    components[0].migrationCount() == 0 && components[1].migrationCount() == 0,
                    "mutation before root check"
                );
            }

            function testRejectsWrongTargetAndSelector() public {
                StandardValidatorUtils.Operation[] memory operations = _operations(1, false);
                operations[0].target = address(components[1]);
                operations[0].expectedCodeHashBefore = address(components[1]).codehash;
                operations[0].expectedCodeHashAfter = address(components[1]).codehash;
                vm.expectPartialRevert(InvalidComponent.selector);
                _stage(release101, operations, 1 days, 2 days);

                operations = _operations(1, false);
                operations[0].data = abi.encodeCall(MigrationComponent.authorizeMigrator, (address(migrator)));
                vm.expectPartialRevert(SelectorNotAllowed.selector);
                _stage(release101, operations, 1 days, 2 days);
            }

            function testRejectsValueCodeHashAndStorageViolations() public {
                StandardValidatorUtils.Operation[] memory operations = _operations(1, false);
                operations[0].value = 1 ether + 1;
                vm.expectPartialRevert(InvalidMigrationOperation.selector);
                _stage(release101, operations, 1 days, 2 days);

                operations = _operations(1, false);
                operations[0].expectedCodeHashBefore = keccak256("wrong");
                vm.expectPartialRevert(MigrationCodeHashMismatch.selector);
                _stage(release101, operations, 1 days, 2 days);

                operations = _operations(1, false);
                operations[0].expectedStorageLayoutBefore = 2;
                operations[0].expectedStorageLayoutAfter = 2;
                vm.expectPartialRevert(MigrationStorageLayoutMismatch.selector);
                _stage(release101, operations, 1 days, 2 days);
            }

            function testLaterFailureRollsBackEveryEarlierOperation() public {
                StandardValidatorUtils.Operation[] memory operations = _operations(2, false);
                operations[1].data = abi.encodeCall(MigrationComponent.migrate, (uint16(2), true));
                bytes32 migrationId = _stage(release101, operations, 1 days, 2 days);
                vm.warp(block.timestamp + 1 days);
                vm.expectPartialRevert(MigrationCallFailed.selector);
                migrator.executeMigration(migrationId, operations);
                require(
                    components[0].storageLayoutVersion() == 1 && components[0].migrationCount() == 0,
                    "partial first mutation"
                );
                require(
                    components[1].storageLayoutVersion() == 1 && components[1].migrationCount() == 0,
                    "failed target mutation"
                );
                require(migrator.activeMigration() == migrationId, "plan state did not roll back");
            }

            function testPostCallHashMismatchRollsBackTarget() public {
                StandardValidatorUtils.Operation[] memory operations = _operations(1, false);
                operations[0].expectedCodeHashAfter = keccak256("wrong-after");
                bytes32 migrationId = _stage(release101, operations, 1 days, 2 days);
                vm.warp(block.timestamp + 1 days);
                vm.expectPartialRevert(MigrationCodeHashMismatch.selector);
                migrator.executeMigration(migrationId, operations);
                require(
                    components[0].storageLayoutVersion() == 1 && components[0].migrationCount() == 0,
                    "post-check partial"
                );
            }

            function testPostCallLayoutMismatchRollsBackTarget() public {
                StandardValidatorUtils.Operation[] memory operations = _operations(1, false);
                operations[0].data = abi.encodeCall(MigrationComponent.migrate, (uint16(1), false));
                bytes32 migrationId = _stage(release101, operations, 1 days, 2 days);
                vm.warp(block.timestamp + 1 days);
                vm.expectPartialRevert(MigrationStorageLayoutMismatch.selector);
                migrator.executeMigration(migrationId, operations);
                require(
                    components[0].storageLayoutVersion() == 1 && components[0].migrationCount() == 0,
                    "layout post-check partial"
                );
            }

            function testCancellationAndAuthorityAreFailClosed() public {
                StandardValidatorUtils.Operation[] memory operations = _operations(1, false);
                vm.expectPartialRevert(ManagerUnauthorized.selector);
                vm.prank(address(0xBAD));
                migrator.stageMigration(release101, configHash, operations, 1 days, 2 days);
                bytes32 migrationId = _stage(release101, operations, 1 days, 2 days);
                vm.prank(governanceTimelock);
                migrator.cancelMigration(migrationId);
                vm.warp(block.timestamp + 1 days);
                vm.expectPartialRevert(InvalidMigrationState.selector);
                migrator.executeMigration(migrationId, operations);
            }

            function testContainerManifestAndSelectorInventoryIsComplete() public view {
                require(container.roleCount() == Predeploys.COUNT, "role count");
                require(container.currentManifestRoot() != bytes32(0), "root");
                for (uint256 i = 0; i < Predeploys.COUNT; ++i) {
                    bytes32 role = Predeploys.roleAt(i);
                    require(container.componentForRole(role) == manifestComponents[i], "component");
                    bytes4[] memory selectors = container.selectorsForRole(role);
                    require(
                        selectors.length == 1 && selectors[0] == MigrationComponent.migrate.selector,
                        "selector inventory"
                    );
                }
            }

            function testContainerRejectsDirectMigrationCompletion() public {
                vm.expectPartialRevert(ManagerUnauthorized.selector);
                container.completeMigration(keccak256("direct-completion"), release101, configHash);
            }

            function testContainerRejectsGenesisFinalizationAgainstUnregisteredTopology() public {
                vm.expectRevert();
                vm.prank(governanceTimelock);
                container.finalizeGenesis();
                require(!container.genesisFinalized() && container.deploymentId() == bytes32(0), "genesis finalized");
            }

            function testContainerRejectsManifestWithMismatchedImmutableGovernance() public {
                StaticConfig.Config memory config = managerConfig;
                ManagerContainer isolatedContainer = new ManagerContainer(config);
                ManagerMigrator isolatedMigrator = new ManagerMigrator(
                    isolatedContainer, governanceTimelock, address(this), 1 days, 7 days, 1_000_000, 1 ether
                );
                MigrationComponent wrongGovernance = new MigrationComponent(
                    Predeploys.ASSET_REGISTRY, configHash, release100, address(0xBAD), EMERGENCY_COUNCIL
                );
                Preinstalls.ComponentManifest[] memory manifests = new Preinstalls.ComponentManifest[](Predeploys.COUNT);
                bytes4[][] memory allowlists = new bytes4[][](Predeploys.COUNT);
                for (uint256 i = 0; i < Predeploys.COUNT; ++i) {
                    address component = manifestComponents[i];
                    if (i == 1) component = address(wrongGovernance);
                    else if (i == 10) component = address(isolatedContainer);
                    else if (i == 11) component = address(isolatedMigrator);
                    ILayerXComponent attested = ILayerXComponent(component);
                    manifests[i] = Preinstalls.ComponentManifest({
                        role: Predeploys.roleAt(i),
                        component: component,
                        interfaceId: Preinstalls.interfaceId(),
                        runtimeCodeHash: component.codehash,
                        configHash: attested.staticConfigHash(),
                        release: attested.releaseVersion(),
                        storageLayout: attested.storageLayoutVersion()
                    });
                    allowlists[i] = new bytes4[](0);
                }
                vm.expectPartialRevert(InvalidComponent.selector);
                vm.prank(governanceTimelock);
                isolatedContainer.initialize(manifests, allowlists);
            }

            function testContainerRejectsMismatchedEmergencyCouncil() public {
                StaticConfig.Config memory config = managerConfig;
                ManagerContainer isolatedContainer = new ManagerContainer(config);
                ManagerMigrator isolatedMigrator = new ManagerMigrator(
                    isolatedContainer, governanceTimelock, address(this), 1 days, 7 days, 1_000_000, 1 ether
                );
                MigrationComponent wrongEmergency = new MigrationComponent(
                    Predeploys.ASSET_REGISTRY, configHash, release100, governanceTimelock, address(0xBAD)
                );
                (Preinstalls.ComponentManifest[] memory manifests, bytes4[][] memory allowlists) =
                    _isolatedManifest(isolatedContainer, isolatedMigrator, address(wrongEmergency));
                vm.expectPartialRevert(InvalidComponent.selector);
                vm.prank(governanceTimelock);
                isolatedContainer.initialize(manifests, allowlists);
            }

            function testContainerRejectsNonCanonicalAddressReturn() public {
                StaticConfig.Config memory config = managerConfig;
                ManagerContainer isolatedContainer = new ManagerContainer(config);
                ManagerMigrator isolatedMigrator = new ManagerMigrator(
                    isolatedContainer, governanceTimelock, address(this), 1 days, 7 days, 1_000_000, 1 ether
                );
                NonCanonicalAddressComponent malformed =
                    new NonCanonicalAddressComponent(configHash, release100, governanceTimelock, EMERGENCY_COUNCIL);
                (Preinstalls.ComponentManifest[] memory manifests, bytes4[][] memory allowlists) =
                    _isolatedManifest(isolatedContainer, isolatedMigrator, address(malformed));
                vm.expectPartialRevert(InvalidComponent.selector);
                vm.prank(governanceTimelock);
                isolatedContainer.initialize(manifests, allowlists);
            }

            function _stage(
                uint192 targetRelease,
                StandardValidatorUtils.Operation[] memory operations,
                uint64 delay,
                uint64 validity
            ) private returns (bytes32) {
                vm.prank(governanceTimelock);
                return migrator.stageMigration(targetRelease, configHash, operations, delay, validity);
            }

            function _isolatedManifest(
                ManagerContainer isolatedContainer,
                ManagerMigrator isolatedMigrator,
                address replacementAsset
            ) private view returns (Preinstalls.ComponentManifest[] memory manifests, bytes4[][] memory allowlists) {
                manifests = new Preinstalls.ComponentManifest[](Predeploys.COUNT);
                allowlists = new bytes4[][](Predeploys.COUNT);
                for (uint256 i = 0; i < Predeploys.COUNT; ++i) {
                    address component = manifestComponents[i];
                    if (i == 1) component = replacementAsset;
                    else if (i == 10) component = address(isolatedContainer);
                    else if (i == 11) component = address(isolatedMigrator);
                    ILayerXComponent attested = ILayerXComponent(component);
                    manifests[i] = Preinstalls.ComponentManifest({
                        role: Predeploys.roleAt(i),
                        component: component,
                        interfaceId: Preinstalls.interfaceId(),
                        runtimeCodeHash: component.codehash,
                        configHash: attested.staticConfigHash(),
                        release: attested.releaseVersion(),
                        storageLayout: attested.storageLayoutVersion()
                    });
                    allowlists[i] = new bytes4[](0);
                }
            }

            function _config(address governance, address emergency, uint192 configRelease)
                private
                view
                returns (StaticConfig.Config memory)
            {
                StaticConfig.AssetDefinition[] memory assets = new StaticConfig.AssetDefinition[](1);
                assets[0] = StaticConfig.AssetDefinition({
                    assetId: Constants.USDL_ASSET_ID,
                    token: Constants.USDL_TOKEN,
                    tokenDecimals: Constants.USDL_TOKEN_DECIMALS,
                    protocolDecimals: Constants.USDL_PROTOCOL_DECIMALS,
                    minimumDeposit: 1_000_000,
                    custodyCap: 1_000_000_000_000
                });
                return StaticConfig.Config({
                    chainId: block.chainid,
                    protocolVersion: Constants.PROTOCOL_VERSION,
                    releaseVersion: configRelease,
                    governanceTimelock: governance,
                    emergencyCouncil: emergency,
                    genesisManifestDigest: keccak256("manager-genesis-manifest"),
                    genesisCanonicalStateRoot: keccak256("manager-genesis-canonical-state-root"),
                    genesisReceiptRoot: keccak256("manager-genesis-receipt-root"),
                    usdlToken: Constants.USDL_TOKEN,
                    usdlAssetId: Constants.USDL_ASSET_ID,
                    usdlDecimals: Constants.USDL_TOKEN_DECIMALS,
                    usdlProtocolDecimals: Constants.USDL_PROTOCOL_DECIMALS,
                    usdlMinimumDeposit: 1_000_000,
                    usdlCustodyCap: 1_000_000_000_000,
                    challengeWindow: 7 days,
                    checkpointLivenessBound: 1 days,
                    enabledFeatures: 0,
                    assetDefinitionsRoot: StaticConfig.hashAssets(assets)
                });
            }

            function _operations(uint256 count, bool failLast)
                private
                view
                returns (StandardValidatorUtils.Operation[] memory operations)
            {
                operations = new StandardValidatorUtils.Operation[](count);
                for (uint256 i = 0; i < count; ++i) {
                    MigrationComponent component = components[i];
                    uint16 beforeLayout = component.storageLayoutVersion();
                    uint16 afterLayout = beforeLayout + 1;
                    operations[i] = StandardValidatorUtils.Operation({
                        role: Predeploys.roleAt(i + 1),
                        target: address(component),
                        value: 0,
                        gasLimit: 300_000,
                        data: abi.encodeCall(MigrationComponent.migrate, (afterLayout, failLast && i + 1 == count)),
                        expectedCodeHashBefore: address(component).codehash,
                        expectedCodeHashAfter: address(component).codehash,
                        expectedStorageLayoutBefore: beforeLayout,
                        expectedStorageLayoutAfter: afterLayout
                    });
                }
            }
        }
