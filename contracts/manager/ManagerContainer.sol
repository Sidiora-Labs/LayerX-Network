// SPDX-License-Identifier: LicenseRef-Centra-ai-Protocol
pragma solidity ^0.8.24;

import {Predeploys} from "../deployment/Predeploys.sol";
import {Preinstalls} from "../deployment/Preinstalls.sol";
import {StaticConfig} from "../config/StaticConfig.sol";
import {SemverComp} from "../libraries/SemverComp.sol";
import {SafeCall} from "../libraries/SafeCall.sol";
import {LayerXComponent} from "../security/LayerXComponent.sol";
import {
    ManagerUnauthorized,
    ManagerNotInitialized,
    ManagerAlreadyInitialized,
    InvalidManagerConfiguration,
    InvalidComponent,
    SelectorNotAllowed
} from "./BlockErrors.sol";

contract ManagerContainer is LayerXComponent {
    address public immutable governanceTimelock;
    address public immutable emergencyCouncil;
    uint192 public currentRelease;
    bytes32 public currentManifestRoot;
    address public migrator;
    bool public initialized;

    mapping(bytes32 => Preinstalls.ComponentManifest) private manifests;
    mapping(bytes32 => mapping(bytes4 => bool)) private selectorPermission;
    mapping(bytes32 => bytes4[]) private roleSelectors;

    event ManagerInitialized(bytes32 indexed manifestRoot, bytes32 indexed configHash, uint192 release);
    event MigratorSet(address indexed migrator);
    event SystemReleaseAdvanced(
        bytes32 indexed migrationId, uint192 sourceRelease, uint192 targetRelease, bytes32 configHash
    );

    constructor(StaticConfig.Config memory config)
        LayerXComponent(Predeploys.CONTRACTS_MANAGER, StaticConfig.hash(config, block.chainid), config.releaseVersion)
    {
        governanceTimelock = config.governanceTimelock;
        emergencyCouncil = config.emergencyCouncil;
        currentRelease = config.releaseVersion;
    }

    modifier onlyGovernance() {
        if (msg.sender != governanceTimelock) {
            revert ManagerUnauthorized(msg.sender);
        }
        _;
    }

    function initialize(
        Preinstalls.ComponentManifest[] calldata initialManifests,
        bytes4[][] calldata selectorAllowlists
    ) external onlyGovernance {
        if (initialized) revert ManagerAlreadyInitialized();
        if (selectorAllowlists.length != Predeploys.COUNT) {
            revert InvalidManagerConfiguration();
        }
        bytes32 root = Preinstalls.validateComplete(initialManifests);
        _validateTopology(initialManifests);
        for (uint256 i = 0; i < Predeploys.COUNT; ++i) {
            Preinstalls.ComponentManifest calldata manifest = initialManifests[i];
            if (manifest.configHash != staticConfigHash) {
                revert InvalidComponent(manifest.role, manifest.component);
            }
            manifests[manifest.role] = manifest;
            bytes4 previous;
            for (uint256 j = 0; j < selectorAllowlists[i].length; ++j) {
                bytes4 selector = selectorAllowlists[i][j];
                if (selector == bytes4(0) || (j != 0 && selector <= previous)) {
                    revert SelectorNotAllowed(manifest.role, selector);
                }
                selectorPermission[manifest.role][selector] = true;
                roleSelectors[manifest.role].push(selector);
                previous = selector;
            }
        }
        currentManifestRoot = root;
        initialized = true;
        emit ManagerInitialized(root, staticConfigHash, currentRelease);
    }

    function setMigrator(address migrationExecutor) external onlyGovernance {
        if (!initialized) revert ManagerNotInitialized();
        if (migrationExecutor == address(0) || migrationExecutor.code.length == 0) {
            revert InvalidManagerConfiguration();
        }
        migrator = migrationExecutor;
        emit MigratorSet(migrationExecutor);
    }

    function completeMigration(bytes32 migrationId, uint192 targetRelease, bytes32 configHash) external {
        if (!initialized) revert ManagerNotInitialized();
        if (msg.sender != migrator) revert ManagerUnauthorized(msg.sender);
        if (
            migrationId == bytes32(0) || configHash != staticConfigHash
                || !SemverComp.isStrictUpgrade(currentRelease, targetRelease)
        ) {
            revert InvalidManagerConfiguration();
        }
        uint192 source = currentRelease;
        currentRelease = targetRelease;
        emit SystemReleaseAdvanced(migrationId, source, targetRelease, configHash);
    }

    function componentForRole(bytes32 role) public view returns (address) {
        if (!initialized) revert ManagerNotInitialized();
        address component = manifests[role].component;
        if (component == address(0)) revert InvalidComponent(role, component);
        return component;
    }

    function manifestForRole(bytes32 role) external view returns (Preinstalls.ComponentManifest memory) {
        componentForRole(role);
        return manifests[role];
    }

    function manifestAt(uint256 index) external view returns (Preinstalls.ComponentManifest memory) {
        return manifests[Predeploys.roleAt(index)];
    }

    function isSelectorAllowed(bytes32 role, bytes4 selector) public view returns (bool) {
        if (!initialized) revert ManagerNotInitialized();
        return selectorPermission[role][selector];
    }

    function requireAllowed(bytes32 role, address target, bytes4 selector) external view {
        if (componentForRole(role) != target) {
            revert InvalidComponent(role, target);
        }
        if (!isSelectorAllowed(role, selector)) {
            revert SelectorNotAllowed(role, selector);
        }
    }

    function selectorsForRole(bytes32 role) external view returns (bytes4[] memory) {
        componentForRole(role);
        return roleSelectors[role];
    }

    function roleCount() external pure returns (uint256) {
        return Predeploys.COUNT;
    }

    function _validateTopology(Preinstalls.ComponentManifest[] calldata initialManifests) private view {
        address timelock = governanceTimelock;
        if (
            initialManifests[0].component != timelock || initialManifests[10].component != address(this)
                || initialManifests[11].component == address(this)
        ) {
            revert InvalidManagerConfiguration();
        }

        _requireAddress(initialManifests[1], bytes4(keccak256("governance()")), timelock);
        _requireAddress(initialManifests[2], bytes4(keccak256("governance()")), timelock);
        _requireAddress(initialManifests[3], bytes4(keccak256("custodyAuthority()")), timelock);
        _requireAddress(initialManifests[3], bytes4(keccak256("membershipAuthority()")), timelock);
        _requireAddress(initialManifests[5], bytes4(keccak256("governance()")), timelock);
        _requireAddress(initialManifests[6], bytes4(keccak256("governance()")), timelock);
        _requireAddress(initialManifests[8], bytes4(keccak256("governance()")), timelock);
        _requireAddress(initialManifests[11], bytes4(keccak256("governanceTimelock()")), timelock);
        _requireAddress(initialManifests[11], bytes4(keccak256("container()")), address(this));

        address emergency = emergencyCouncil;
        _requireAddress(initialManifests[1], bytes4(keccak256("emergencyCouncil()")), emergency);
        _requireAddress(initialManifests[2], bytes4(keccak256("emergencyCouncil()")), emergency);
        _requireAddress(initialManifests[5], bytes4(keccak256("emergencyCouncil()")), emergency);
        _requireAddress(initialManifests[6], bytes4(keccak256("emergencyCouncil()")), emergency);
        _requireAddress(initialManifests[8], bytes4(keccak256("emergencyCouncil()")), emergency);

        _requireAddress(initialManifests[2], bytes4(keccak256("assetRegistry()")), initialManifests[1].component);
        _requireAddress(initialManifests[4], bytes4(keccak256("guarantorEligibility()")), initialManifests[3].component);
        _requireAddress(initialManifests[5], bytes4(keccak256("registry()")), initialManifests[4].component);
        _requireAddress(initialManifests[5], bytes4(keccak256("guarantorBond()")), initialManifests[3].component);
        _requireAddress(initialManifests[7], bytes4(keccak256("registry()")), initialManifests[4].component);
        _requireAddress(initialManifests[7], bytes4(keccak256("challengeManager()")), initialManifests[5].component);
        _requireAddress(initialManifests[7], bytes4(keccak256("nullifierRegistry()")), initialManifests[6].component);
        _requireAddress(initialManifests[7], bytes4(keccak256("vault()")), initialManifests[2].component);
        _requireAddress(initialManifests[8], bytes4(keccak256("registry()")), initialManifests[4].component);
        _requireAddress(initialManifests[8], bytes4(keccak256("challengeManager()")), initialManifests[5].component);
        _requireAddress(initialManifests[8], bytes4(keccak256("nullifierRegistry()")), initialManifests[6].component);
        _requireAddress(initialManifests[8], bytes4(keccak256("vault()")), initialManifests[2].component);
        _requireAddress(initialManifests[9], bytes4(keccak256("registry()")), initialManifests[4].component);
        _requireAddress(initialManifests[9], bytes4(keccak256("vault()")), initialManifests[2].component);
        _requireAddress(initialManifests[9], bytes4(keccak256("withdrawalClaims()")), initialManifests[7].component);
        _requireAddress(initialManifests[12], bytes4(keccak256("checkpointRegistry()")), initialManifests[4].component);
        _requireAddress(initialManifests[12], bytes4(keccak256("guarantorBond()")), initialManifests[3].component);
        _requireAddress(initialManifests[12], bytes4(keccak256("vault()")), initialManifests[2].component);
        _requireAddress(initialManifests[12], bytes4(keccak256("challengeManager()")), initialManifests[5].component);
        _requireAddress(
            initialManifests[12], bytes4(keccak256("withdrawalNullifiers()")), initialManifests[6].component
        );
        _requireAddress(initialManifests[12], bytes4(keccak256("withdrawalClaims()")), initialManifests[7].component);
    }

    function _requireAddress(Preinstalls.ComponentManifest calldata manifest, bytes4 selector, address expected)
        private
        view
    {
        SafeCall.CallResult memory result =
            SafeCall.staticCall(manifest.component, abi.encodeWithSelector(selector), 50_000, 32);
        if (!result.success || result.returnDataSize != 32) revert InvalidComponent(manifest.role, manifest.component);
        bytes memory data = result.returnData;
        uint256 word;
        assembly ("memory-safe") {
            word := mload(add(data, 32))
        }
        if (word > type(uint160).max) revert InvalidComponent(manifest.role, manifest.component);
        address actual;
        assembly ("memory-safe") {
            actual := word
        }
        if (actual != expected) revert InvalidComponent(manifest.role, manifest.component);
    }
}
