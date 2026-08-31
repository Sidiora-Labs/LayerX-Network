// SPDX-License-Identifier: LicenseRef-Centra-ai-Protocol
pragma solidity ^0.8.24;

import {Features} from "../contracts/config/Features.sol";
import {StaticConfig} from "../contracts/config/StaticConfig.sol";
import {Blueprint} from "../contracts/deployment/Blueprint.sol";
import {Predeploys} from "../contracts/deployment/Predeploys.sol";
import {ILayerXComponent, Preinstalls} from "../contracts/deployment/Preinstalls.sol";
import {LayerXTimelock} from "../contracts/governance/LayerXTimelock.sol";
import {ManagerContainer} from "../contracts/manager/ManagerContainer.sol";
import {ManagerMigrator} from "../contracts/manager/ManagerMigrator.sol";
import {Constants} from "../contracts/libraries/Constants.sol";
import {SemverComp} from "../contracts/libraries/SemverComp.sol";

interface DeploymentVm {
    function etch(address account, bytes calldata code) external;
    function expectPartialRevert(bytes4 selector) external;
    function prank(address sender) external;
}

contract DeploymentHarness {
    function parseVersion(string calldata version) external pure returns (uint192) {
        return SemverComp.parseRelease(version);
    }

    function compareVersions(uint192 first, uint192 second) external pure returns (int8) {
        return SemverComp.compare(first, second);
    }

    function validateFeatures(uint256 features) external pure {
        Features.validate(features);
    }

    function hashAssets(StaticConfig.AssetDefinition[] memory assets) external pure returns (bytes32) {
        return StaticConfig.hashAssets(assets);
    }

    function configHash(StaticConfig.Config memory config, uint256 actualChainId) external pure returns (bytes32) {
        return StaticConfig.hash(config, actualChainId);
    }

    function roleAt(uint256 index) external pure returns (bytes32) {
        return Predeploys.roleAt(index);
    }

    function predeployCount() external pure returns (uint256) {
        return Predeploys.COUNT;
    }

    function componentInterfaceId() external pure returns (bytes4) {
        return Preinstalls.interfaceId();
    }

    function validateComponent(Preinstalls.ComponentManifest memory manifest) external view {
        Preinstalls.validate(manifest);
    }

    function validateManifest(Preinstalls.ComponentManifest[] memory manifests) external view returns (bytes32) {
        return Preinstalls.validateComplete(manifests);
    }
}

contract BlueprintTarget {
    uint256 public immutable value;

    constructor(uint256 initialValue) {
        value = initialValue;
    }
}

contract ComponentAttestation is ILayerXComponent {
    bytes32 public immutable override componentRole;
    bytes32 public immutable override staticConfigHash;
    uint192 public immutable override releaseVersion;
    uint16 public immutable override storageLayoutVersion;

    constructor(bytes32 role, bytes32 configHash, uint192 release, uint16 layout) {
        componentRole = role;
        staticConfigHash = configHash;
        releaseVersion = release;
        storageLayoutVersion = layout;
    }
}

    contract TopologyAttestation is ILayerXComponent {
        bytes32 public immutable override componentRole;
        bytes32 public immutable override staticConfigHash;
        uint192 public immutable override releaseVersion;
        uint16 public constant override storageLayoutVersion = Constants.STORAGE_LAYOUT_VERSION;
        address public immutable governance;
        address public immutable emergencyCouncil;
        address public immutable custodyAuthority;
        address public immutable membershipAuthority;
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

        constructor(bytes32 role, bytes32 configHash, uint192 release, address timelock, address emergencyAuthority) {
            componentRole = role;
            staticConfigHash = configHash;
            releaseVersion = release;
            governance = timelock;
            emergencyCouncil = emergencyAuthority;
            custodyAuthority = timelock;
            membershipAuthority = timelock;
        }

        function configure(address[] calldata topology) external {
            require(topology.length == 7 && assetRegistry == address(0), "topology");
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
        }
    }

        contract ContractDeploymentTest {
            DeploymentVm private constant vm = DeploymentVm(address(uint160(uint256(keccak256("hevm cheat code")))));
            DeploymentHarness private harness;
            uint192 private release;

            function setUp() public {
                harness = new DeploymentHarness();
                release = harness.parseVersion("1.0.0");
            }

            function testStrictStableSemverComparison() public view {
                uint192 v100 = harness.parseVersion("1.0.0");
                uint192 v101 = harness.parseVersion("1.0.1");
                uint192 v110 = harness.parseVersion("1.1.0");
                uint192 v200 = harness.parseVersion("2.0.0");
                require(harness.compareVersions(v100, v101) == -1, "patch");
                require(harness.compareVersions(v101, v110) == -1, "minor");
                require(harness.compareVersions(v110, v200) == -1, "major");
                require(harness.compareVersions(v200, v200) == 0, "equal");
            }

            function testSemverRejectsLeadingZeroAndPrerelease() public {
                vm.expectPartialRevert(SemverComp.InvalidSemanticVersion.selector);
                harness.parseVersion("01.0.0");
                vm.expectPartialRevert(SemverComp.InvalidSemanticVersion.selector);
                harness.parseVersion("1.0.0-alpha");
                vm.expectPartialRevert(SemverComp.InvalidSemanticVersion.selector);
                harness.parseVersion("1.0");
            }

            function testFeaturesRejectForeignAndUnknownFacilities() public {
                harness.validateFeatures(Features.ERC20_CUSTODY | Features.EMERGENCY_EXIT);
                vm.expectPartialRevert(Features.ForeignChainFacility.selector);
                harness.validateFeatures(Features.ARBITRUM_PRECOMPILES);
                vm.expectPartialRevert(Features.UnsupportedFeature.selector);
                harness.validateFeatures(1 << 100);
            }

            function testStaticConfigBindsAssetsChainAndAuthorities() public view {
                StaticConfig.AssetDefinition[] memory assets = _assets();
                bytes32 assetsRoot = harness.hashAssets(assets);
                StaticConfig.Config memory config = _config(assetsRoot);
                bytes32 first = harness.configHash(config, block.chainid);
                config.genesisReceiptRoot = keccak256("different-genesis-receipt-root");
                bytes32 receiptRootChanged = harness.configHash(config, block.chainid);
                require(first != receiptRootChanged, "genesis receipt root not bound");
                config.challengeWindow += 1;
                bytes32 second = harness.configHash(config, block.chainid);
                require(receiptRootChanged != second, "config field not bound");
            }

            function testStaticConfigRejectsWrongChainAndUnorderedAssets() public {
                StaticConfig.AssetDefinition[] memory assets = _assets();
                bytes32 assetsRoot = harness.hashAssets(assets);
                StaticConfig.Config memory config = _config(assetsRoot);
                vm.expectPartialRevert(StaticConfig.StaticConfigWrongChain.selector);
                harness.configHash(config, block.chainid + 1);
                StaticConfig.AssetDefinition memory temporary = assets[0];
                assets[0] = assets[1];
                assets[1] = temporary;
                vm.expectPartialRevert(StaticConfig.AssetDefinitionsNotOrdered.selector);
                harness.hashAssets(assets);
            }

            function testBlueprintDeploysAtPredictedAddressAndIsIdempotent() public {
                Blueprint blueprint = new Blueprint(address(this), _blueprintConfig());
                _deployTimelock(blueprint);
                bytes32 role = Predeploys.VAULT;
                bytes memory initCode = abi.encodePacked(type(BlueprintTarget).creationCode, abi.encode(uint256(7)));
                BlueprintTarget runtimeReference = new BlueprintTarget(7);
                bytes32 expectedRuntime = address(runtimeReference).codehash;
                address predicted = blueprint.predict(role, keccak256(initCode));
                address deployed = blueprint.deploy(role, initCode, expectedRuntime);
                require(deployed == predicted && deployed.codehash == expectedRuntime, "deterministic deployment");
                require(BlueprintTarget(deployed).value() == 7, "constructor binding");
                require(blueprint.deploy(role, initCode, expectedRuntime) == deployed, "idempotence");
            }

            function testBlueprintRejectsOccupiedAndWrongRuntimeCode() public {
                Blueprint blueprint = new Blueprint(address(this), _blueprintConfig());
                _deployTimelock(blueprint);
                BlueprintTarget runtimeReference = new BlueprintTarget(9);
                bytes memory initCode = abi.encodePacked(type(BlueprintTarget).creationCode, abi.encode(uint256(9)));
                bytes32 initCodeHash = keccak256(initCode);
                address predicted = blueprint.predict(Predeploys.ASSET_REGISTRY, initCodeHash);
                vm.etch(predicted, hex"6000");
                vm.expectPartialRevert(Blueprint.DeploymentCollision.selector);
                blueprint.deploy(Predeploys.ASSET_REGISTRY, initCode, address(runtimeReference).codehash);
                vm.expectPartialRevert(Blueprint.DeploymentCollision.selector);
                blueprint.deploy(Predeploys.GUARANTOR_BOND, initCode, keccak256("wrong"));
            }

            function testBlueprintRejectsNonManager() public {
                Blueprint blueprint = new Blueprint(address(this), _blueprintConfig());
                (bytes memory initCode, bytes32 runtimeCodeHash) = _timelockInitCode(blueprint);
                vm.expectPartialRevert(Blueprint.ManagerOnly.selector);
                vm.prank(address(0xBAD));
                blueprint.deployTimelock(initCode, runtimeCodeHash);
            }

            function testBlueprintDerivesCanonicalTimelockBeforeStaticConfigHash() public {
                StaticConfig.Config memory config = _blueprintConfig();
                Blueprint blueprint = new Blueprint(address(this), config);
                address predicted = blueprint.predictTimelock();
                require(predicted != address(0), "timelock prediction");
                config.governanceTimelock = predicted;
                require(blueprint.staticConfigHash() == harness.configHash(config, block.chainid), "config prediction");
                require(blueprint.releaseVersion() == config.releaseVersion, "release prediction");
            }

            function testBlueprintRequiresTimelockAsFirstAndOnlyBootstrapDeployment() public {
                Blueprint blueprint = new Blueprint(address(this), _blueprintConfig());
                bytes memory targetInitCode = abi.encodePacked(
                    type(BlueprintTarget).creationCode, abi.encode(uint256(11))
                );
                BlueprintTarget targetReference = new BlueprintTarget(11);
                vm.expectPartialRevert(Blueprint.InvalidBlueprint.selector);
                blueprint.deploy(Predeploys.VAULT, targetInitCode, address(targetReference).codehash);

                (bytes memory timelockInitCode, bytes32 timelockRuntimeCodeHash) = _timelockInitCode(blueprint);
                address timelockAddress = blueprint.deployTimelock(timelockInitCode, timelockRuntimeCodeHash);
                require(timelockAddress == blueprint.predictTimelock(), "timelock address");
                require(blueprint.deploymentForRole(Predeploys.TIMELOCK) == timelockAddress, "timelock role");
                require(
                    LayerXTimelock(payable(timelockAddress)).staticConfigHash() == blueprint.staticConfigHash(),
                    "config"
                );

                require(
                    blueprint.deployTimelock(timelockInitCode, timelockRuntimeCodeHash) == timelockAddress,
                    "timelock idempotence"
                );
                bytes memory changedInitCode = abi.encodePacked(
                    type(LayerXTimelock).creationCode,
                    abi.encode(
                        uint64(2 days),
                        uint64(7 days),
                        address(0xA11CE),
                        address(this),
                        address(this),
                        uint256(1 ether),
                        blueprint.staticConfigHash(),
                        release
                    )
                );
                vm.expectPartialRevert(Blueprint.DeploymentCollision.selector);
                blueprint.deployTimelock(changedInitCode, timelockRuntimeCodeHash);
                vm.expectPartialRevert(Blueprint.InvalidBlueprint.selector);
                blueprint.deploy(Predeploys.TIMELOCK, timelockInitCode, timelockRuntimeCodeHash);
            }

            function testBlueprintTimelockWrongRuntimeHashRollsBackFirstChildNonce() public {
                Blueprint blueprint = new Blueprint(address(this), _blueprintConfig());
                (bytes memory initCode, bytes32 runtimeCodeHash) = _timelockInitCode(blueprint);
                address predicted = blueprint.predictTimelock();
                vm.expectPartialRevert(Blueprint.DeploymentCollision.selector);
                blueprint.deployTimelock(initCode, keccak256("wrong-timelock-runtime"));
                require(predicted.code.length == 0, "failed child persisted");
                require(blueprint.deploymentForRole(Predeploys.TIMELOCK) == address(0), "failed role persisted");
                require(blueprint.deployTimelock(initCode, runtimeCodeHash) == predicted, "nonce not rolled back");
            }

            function testBlueprintRejectsConfiguredGovernanceAndIncompleteSeal() public {
                StaticConfig.Config memory configured = _config(harness.hashAssets(_assets()));
                vm.expectPartialRevert(Blueprint.InvalidBlueprint.selector);
                new Blueprint(address(this), configured);

                Blueprint blueprint = new Blueprint(address(this), _blueprintConfig());
                _deployTimelock(blueprint);
                vm.expectPartialRevert(Blueprint.InvalidBlueprint.selector);
                blueprint.seal();
            }

            function testBlueprintSealRejectsArbitraryBytecodeDespiteMatchingExpectedRuntimeHash() public {
                Blueprint blueprint = new Blueprint(address(this), _blueprintConfig());
                _deployTimelock(blueprint);
                _populateSealMappings(blueprint, 1);
                vm.expectPartialRevert(Blueprint.InvalidBlueprint.selector);
                blueprint.seal();
            }

            function testBlueprintSealRejectsWrongRoleAttestation() public {
                _assertSealAttestationFault(2);
            }

            function testBlueprintSealRejectsWrongConfigAttestation() public {
                _assertSealAttestationFault(3);
            }

            function testBlueprintSealRejectsWrongReleaseAttestation() public {
                _assertSealAttestationFault(4);
            }

            function testBlueprintSealRejectsZeroStorageLayoutAttestation() public {
                _assertSealAttestationFault(5);
            }

            function testBlueprintSealRejectsNoncanonicalStorageLayoutAttestation() public {
                _assertSealAttestationFault(6);
            }

            function testBlueprintSealRejectsUninitializedManager() public {
                Blueprint blueprint = new Blueprint(address(this), _blueprintConfig());
                _deployTimelock(blueprint);
                for (uint256 i = 1; i < Predeploys.COUNT; ++i) {
                    bytes32 role = Predeploys.roleAt(i);
                    if (role == Predeploys.CONTRACTS_MANAGER) _deployManager(blueprint);
                    else _deployAttestation(blueprint, role, role, blueprint.staticConfigHash(), release, 1);
                }
                vm.expectPartialRevert(Blueprint.InvalidBlueprint.selector);
                blueprint.seal();
            }

            function testBlueprintSealRejectsManagerManifestMappingDisagreement() public {
                Blueprint blueprint = new Blueprint(address(this), _blueprintConfig());
                address timelock = address(_deployTimelock(blueprint));
                for (uint256 i = 1; i < 10; ++i) {
                    bytes32 role = Predeploys.roleAt(i);
                    _deployAttestation(blueprint, role, role, blueprint.staticConfigHash(), release, 1);
                }
                ManagerContainer managerContainer = _deployManager(blueprint);
                ManagerMigrator migrator = _deployMigrator(blueprint, managerContainer);
                _deployAttestation(
                    blueprint,
                    Predeploys.CUSTODY_TOPOLOGY,
                    Predeploys.CUSTODY_TOPOLOGY,
                    blueprint.staticConfigHash(),
                    release,
                    1
                );

                (Preinstalls.ComponentManifest[] memory manifests, bytes4[][] memory allowlists) =
                    _mismatchedManifest(blueprint, managerContainer, migrator);
                vm.prank(timelock);
                managerContainer.initialize(manifests, allowlists);
                vm.expectPartialRevert(Blueprint.InvalidBlueprint.selector);
                blueprint.seal();
            }

            function testBlueprintRejectsTimelockWithoutComponentAttestation() public {
                Blueprint blueprint = new Blueprint(address(this), _blueprintConfig());
                bytes memory initCode = abi.encodePacked(type(BlueprintTarget).creationCode, abi.encode(uint256(13)));
                BlueprintTarget runtimeReference = new BlueprintTarget(13);
                vm.expectPartialRevert(Blueprint.InvalidBlueprint.selector);
                blueprint.deployTimelock(initCode, address(runtimeReference).codehash);
                require(blueprint.predictTimelock().code.length == 0, "unattested timelock persisted");
                _deployTimelock(blueprint);
            }

            function testPreinstallManifestIsExhaustiveAndAttested() public {
                Preinstalls.ComponentManifest[] memory manifests = _manifest();
                bytes32 root = harness.validateManifest(manifests);
                require(root != bytes32(0), "manifest root");
                manifests[4].runtimeCodeHash = keccak256("wrong");
                vm.expectPartialRevert(Preinstalls.InvalidComponentManifest.selector);
                harness.validateManifest(manifests);
            }

            function testPreinstallManifestRejectsDuplicateAddress() public {
                Preinstalls.ComponentManifest[] memory manifests = _manifest();
                manifests[1].component = manifests[0].component;
                vm.expectPartialRevert(Preinstalls.DuplicateComponentAddress.selector);
                harness.validateManifest(manifests);
            }

            function testPredeployRolesAreExhaustiveAndUnique() public view {
                uint256 count = harness.predeployCount();
                require(count == 13, "role count");
                for (uint256 i = 0; i < count; ++i) {
                    bytes32 role = harness.roleAt(i);
                    require(role != bytes32(0), "empty role");
                    for (uint256 j = 0; j < i; ++j) {
                        require(role != harness.roleAt(j), "duplicate role");
                    }
                }
            }

            function _assets() private pure returns (StaticConfig.AssetDefinition[] memory assets) {
                assets = new StaticConfig.AssetDefinition[](2);
                assets[0] = StaticConfig.AssetDefinition({
                    assetId: bytes32(uint256(1)),
                    token: address(0x1001),
                    tokenDecimals: 6,
                    protocolDecimals: 18,
                    minimumDeposit: 1_000_000,
                    custodyCap: 1_000_000_000_000
                });
                assets[1] = StaticConfig.AssetDefinition({
                    assetId: bytes32(uint256(2)),
                    token: address(0x1002),
                    tokenDecimals: 18,
                    protocolDecimals: 18,
                    minimumDeposit: 1 ether,
                    custodyCap: 1_000_000 ether
                });
            }

            function _config(bytes32 assetRoot) private view returns (StaticConfig.Config memory) {
                return StaticConfig.Config({
                    chainId: block.chainid,
                    protocolVersion: Constants.PROTOCOL_VERSION,
                    releaseVersion: release,
                    governanceTimelock: address(0x1000),
                    emergencyCouncil: address(0x2000),
                    genesisReceiptRoot: keccak256("genesis-receipt-root"),
                    challengeWindow: 7 days,
                    checkpointLivenessBound: 1 days,
                    enabledFeatures: Features.ERC20_CUSTODY | Features.CHECKPOINT_CHALLENGES
                        | Features.WITHDRAWAL_CLAIMS | Features.EMERGENCY_EXIT | Features.RESERVE_RECONCILIATION,
                    assetDefinitionsRoot: assetRoot
                });
            }

            function _blueprintConfig() private view returns (StaticConfig.Config memory config) {
                config = _config(harness.hashAssets(_assets()));
                config.governanceTimelock = address(0);
            }

            function _deployTimelock(Blueprint blueprint) private returns (LayerXTimelock timelock) {
                (bytes memory initCode, bytes32 runtimeCodeHash) = _timelockInitCode(blueprint);
                timelock = LayerXTimelock(payable(blueprint.deployTimelock(initCode, runtimeCodeHash)));
            }

            function _timelockInitCode(Blueprint blueprint)
                private
                returns (bytes memory initCode, bytes32 runtimeCodeHash)
            {
                bytes memory arguments = abi.encode(
                    uint64(2 days),
                    uint64(7 days),
                    address(this),
                    address(this),
                    address(this),
                    uint256(1 ether),
                    blueprint.staticConfigHash(),
                    release
                );
                initCode = abi.encodePacked(type(LayerXTimelock).creationCode, arguments);
                LayerXTimelock runtimeReference = new LayerXTimelock(
                    2 days,
                    7 days,
                    address(this),
                    address(this),
                    address(this),
                    1 ether,
                    blueprint.staticConfigHash(),
                    release
                );
                runtimeCodeHash = address(runtimeReference).codehash;
            }

            function _populateSealMappings(Blueprint blueprint, uint8 fault) private {
                for (uint256 i = 1; i < Predeploys.COUNT; ++i) {
                    bytes32 role = Predeploys.roleAt(i);
                    if (i == 1 && fault == 1) {
                        bytes memory initCode =
                            abi.encodePacked(type(BlueprintTarget).creationCode, abi.encode(uint256(31)));
                        BlueprintTarget runtimeReference = new BlueprintTarget(31);
                        blueprint.deploy(role, initCode, address(runtimeReference).codehash);
                        continue;
                    }
                    bytes32 attestedRole = i == 1 && fault == 2 ? Predeploys.VAULT : role;
                    bytes32 configHash = i == 1 && fault == 3 ? keccak256("wrong-config") : blueprint.staticConfigHash();
                    uint192 componentRelease = i == 1 && fault == 4 ? release + 1 : release;
                    uint16 layout = i == 1 && fault == 5 ? 0 : Constants.STORAGE_LAYOUT_VERSION;
                    if (i == 1 && fault == 6) layout = Constants.STORAGE_LAYOUT_VERSION + 1;
                    _deployAttestation(blueprint, role, attestedRole, configHash, componentRelease, layout);
                }
            }

            function _assertSealAttestationFault(uint8 fault) private {
                Blueprint blueprint = new Blueprint(address(this), _blueprintConfig());
                _deployTimelock(blueprint);
                _populateSealMappings(blueprint, fault);
                vm.expectPartialRevert(Blueprint.InvalidBlueprint.selector);
                blueprint.seal();
            }

            function _deployAttestation(
                Blueprint blueprint,
                bytes32 deploymentRole,
                bytes32 attestedRole,
                bytes32 configHash,
                uint192 componentRelease,
                uint16 layout
            ) private returns (address component) {
                bytes memory initCode = abi.encodePacked(
                    type(ComponentAttestation).creationCode,
                    abi.encode(attestedRole, configHash, componentRelease, layout)
                );
                ComponentAttestation runtimeReference =
                    new ComponentAttestation(attestedRole, configHash, componentRelease, layout);
                component = blueprint.deploy(deploymentRole, initCode, address(runtimeReference).codehash);
            }

            function _deployManager(Blueprint blueprint) private returns (ManagerContainer managerContainer) {
                StaticConfig.Config memory config = _resolvedConfig(blueprint);
                bytes memory initCode = abi.encodePacked(type(ManagerContainer).creationCode, abi.encode(config));
                ManagerContainer runtimeReference = new ManagerContainer(config);
                managerContainer = ManagerContainer(
                    blueprint.deploy(Predeploys.CONTRACTS_MANAGER, initCode, address(runtimeReference).codehash)
                );
            }

            function _deployMigrator(Blueprint blueprint, ManagerContainer managerContainer)
                private
                returns (ManagerMigrator migrator)
            {
                bytes memory arguments = abi.encode(
                    managerContainer,
                    blueprint.governanceTimelock(),
                    address(this),
                    uint64(1 days),
                    uint64(7 days),
                    uint64(1_000_000),
                    uint256(1 ether)
                );
                ManagerMigrator runtimeReference = new ManagerMigrator(
                    managerContainer, blueprint.governanceTimelock(), address(this), 1 days, 7 days, 1_000_000, 1 ether
                );
                migrator = ManagerMigrator(
                    payable(blueprint.deploy(
                            Predeploys.MANAGER_MIGRATOR,
                            abi.encodePacked(type(ManagerMigrator).creationCode, arguments),
                            address(runtimeReference).codehash
                        ))
                );
            }

            function _mismatchedManifest(
                Blueprint blueprint,
                ManagerContainer managerContainer,
                ManagerMigrator migrator
            ) private returns (Preinstalls.ComponentManifest[] memory manifests, bytes4[][] memory allowlists) {
                TopologyAttestation[] memory topologyComponents = new TopologyAttestation[](Predeploys.COUNT);
                address[] memory topology = new address[](7);
                for (uint256 i = 1; i < Predeploys.COUNT; ++i) {
                    if (i == 10 || i == 11) continue;
                    topologyComponents[i] = new TopologyAttestation(
                        Predeploys.roleAt(i),
                        blueprint.staticConfigHash(),
                        release,
                        blueprint.governanceTimelock(),
                        blueprint.emergencyCouncil()
                    );
                }
                topology[0] = address(topologyComponents[1]);
                topology[1] = address(topologyComponents[2]);
                topology[2] = address(topologyComponents[3]);
                topology[3] = address(topologyComponents[4]);
                topology[4] = address(topologyComponents[5]);
                topology[5] = address(topologyComponents[6]);
                topology[6] = address(topologyComponents[7]);
                for (uint256 i = 1; i < Predeploys.COUNT; ++i) {
                    if (i != 10 && i != 11) topologyComponents[i].configure(topology);
                }

                manifests = new Preinstalls.ComponentManifest[](Predeploys.COUNT);
                allowlists = new bytes4[][](Predeploys.COUNT);
                for (uint256 i = 0; i < Predeploys.COUNT; ++i) {
                    address component;
                    if (i == 0) component = blueprint.deploymentForRole(Predeploys.TIMELOCK);
                    else if (i == 10) component = address(managerContainer);
                    else if (i == 11) component = address(migrator);
                    else component = address(topologyComponents[i]);
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

            function _resolvedConfig(Blueprint blueprint) private view returns (StaticConfig.Config memory config) {
                config = _blueprintConfig();
                config.governanceTimelock = blueprint.governanceTimelock();
            }

            function _manifest() private returns (Preinstalls.ComponentManifest[] memory manifests) {
                manifests = new Preinstalls.ComponentManifest[](Predeploys.COUNT);
                bytes32 configHash = keccak256("manifest-config");
                bytes4 expectedInterface = harness.componentInterfaceId();
                for (uint256 i = 0; i < manifests.length; ++i) {
                    bytes32 role = harness.roleAt(i);
                    ComponentAttestation component =
                        new ComponentAttestation(role, configHash, release, Constants.STORAGE_LAYOUT_VERSION);
                    manifests[i] = Preinstalls.ComponentManifest({
                        role: role,
                        component: address(component),
                        interfaceId: expectedInterface,
                        runtimeCodeHash: address(component).codehash,
                        configHash: configHash,
                        release: release,
                        storageLayout: Constants.STORAGE_LAYOUT_VERSION
                    });
                }
            }
        }
