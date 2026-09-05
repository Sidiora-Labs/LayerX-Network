// SPDX-License-Identifier: LicenseRef-Centra-ai-Protocol
pragma solidity ^0.8.24;

import {Predeploys} from "../deployment/Predeploys.sol";
import {Preinstalls} from "../deployment/Preinstalls.sol";
import {StaticConfig} from "../config/StaticConfig.sol";
import {SemverComp} from "../libraries/SemverComp.sol";
import {SafeCall} from "../libraries/SafeCall.sol";
import {SafeTransfer} from "../libraries/SafeTransfer.sol";
import {Constants} from "../libraries/Constants.sol";
import {ILayerXAssetRegistry} from "../interfaces/ILayerXAssetRegistry.sol";
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
    uint16 public immutable protocolVersion;
    address public immutable governanceTimelock;
    address public immutable emergencyCouncil;
    bytes32 public immutable genesisManifestDigest;
    bytes32 public immutable genesisCanonicalStateRoot;
    bytes32 public immutable genesisReceiptRoot;
    address public immutable usdlToken;
    bytes32 public immutable usdlAssetId;
    uint8 public immutable usdlDecimals;
    uint128 public immutable usdlMinimumDeposit;
    uint128 public immutable usdlCustodyCap;
    uint192 public currentRelease;
    bytes32 public currentManifestRoot;
    address public migrator;
    bool public initialized;
    bool public genesisFinalized;
    bytes32 public deploymentId;

    mapping(bytes32 => Preinstalls.ComponentManifest) private manifests;
    mapping(bytes32 => mapping(bytes4 => bool)) private selectorPermission;
    mapping(bytes32 => bytes4[]) private roleSelectors;

    event ManagerInitialized(bytes32 indexed manifestRoot, bytes32 indexed configHash, uint192 release);
    event MigratorSet(address indexed migrator);
    event GenesisDeploymentFinalized(
        bytes32 indexed deploymentId,
        bytes32 indexed genesisCheckpointId,
        bytes32 indexed bondedSetCommitment,
        uint64 bondedSetVersion
    );
    event SystemReleaseAdvanced(
        bytes32 indexed migrationId, uint192 sourceRelease, uint192 targetRelease, bytes32 configHash
    );

    constructor(StaticConfig.Config memory config)
        LayerXComponent(
            Predeploys.CONTRACTS_MANAGER,
            StaticConfig.hashForProtocol(config, block.chainid, config.protocolVersion),
            config.releaseVersion
        )
    {
        governanceTimelock = config.governanceTimelock;
        protocolVersion = config.protocolVersion;
        emergencyCouncil = config.emergencyCouncil;
        genesisManifestDigest = config.genesisManifestDigest;
        genesisCanonicalStateRoot = config.genesisCanonicalStateRoot;
        genesisReceiptRoot = config.genesisReceiptRoot;
        usdlToken = config.usdlToken;
        usdlAssetId = config.usdlAssetId;
        usdlDecimals = config.usdlDecimals;
        usdlMinimumDeposit = config.usdlMinimumDeposit;
        usdlCustodyCap = config.usdlCustodyCap;
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

    function finalizeGenesis() external onlyGovernance {
        if (!initialized || genesisFinalized) revert InvalidManagerConfiguration();
        address registryAddress = componentForRole(Predeploys.ASSET_REGISTRY);
        address vaultAddress = componentForRole(Predeploys.VAULT);
        address bondAddress = componentForRole(Predeploys.GUARANTOR_BOND);
        address checkpointAddress = componentForRole(Predeploys.CHECKPOINT_REGISTRY);
        if (migrator != componentForRole(Predeploys.MANAGER_MIGRATOR)) revert InvalidManagerConfiguration();

        ILayerXAssetRegistry.AssetConfig memory asset = ILayerXAssetRegistry(registryAddress).asset(usdlAssetId);
        SafeCall.CallResult memory decimalsResult =
            SafeCall.staticCall(usdlToken, abi.encodeWithSelector(bytes4(keccak256("decimals()"))), 50_000, 32);
        if (
            !decimalsResult.success || decimalsResult.returnDataSize != 32
                || abi.decode(decimalsResult.returnData, (uint256)) != usdlDecimals
        ) revert InvalidManagerConfiguration();
        if (
            asset.token != usdlToken || asset.decimals != usdlDecimals || asset.minimumDeposit != usdlMinimumDeposit
                || asset.custodyCap != usdlCustodyCap || !asset.enabled || asset.paused
        ) revert InvalidManagerConfiguration();

        IGenesisVault vaultComponent = IGenesisVault(vaultAddress);
        IGenesisBond bondComponent = IGenesisBond(bondAddress);
        IGenesisCheckpointRegistry checkpointComponent = IGenesisCheckpointRegistry(checkpointAddress);
        bytes32 bondedSetCommitment = bondComponent.genesisBondedSetCommitment();
        uint64 bondedSetVersion = bondComponent.genesisBondedSetVersion();
        bytes32 genesisCheckpoint = checkpointComponent.genesisCheckpointId();
        if (
            vaultComponent.assetRegistry() != registryAddress || vaultComponent.guarantorBond() != bondAddress
                || bondComponent.vault() != vaultAddress || bondComponent.bondToken() != usdlToken
                || bondComponent.assetId() != usdlAssetId || bondedSetCommitment == bytes32(0) || bondedSetVersion == 0
                || bondedSetVersion != bondComponent.membershipVersion() || bondComponent.slashedBalance() != 0
                || SafeTransfer.balanceOf(usdlToken, bondAddress) != bondComponent.totalBonded()
                || bondComponent.custodiedValue() != vaultComponent.totalCustodied(usdlAssetId)
                || checkpointComponent.protocolVersion() != protocolVersion
                || checkpointComponent.genesisManifestDigest() != genesisManifestDigest
                || checkpointComponent.genesisCanonicalStateRoot() != genesisCanonicalStateRoot
                || checkpointComponent.genesisReceiptRoot() != genesisReceiptRoot || genesisCheckpoint == bytes32(0)
                || checkpointComponent.derivedGenesisCheckpointId() != genesisCheckpoint
                || checkpointComponent.staticConfigHash() != staticConfigHash
        ) revert InvalidManagerConfiguration();

        bytes32 identifier = sha256(
            abi.encode(
                "LXP/Paxeer/genesis-deployment/v2",
                block.chainid,
                staticConfigHash,
                currentManifestRoot,
                genesisManifestDigest,
                genesisCanonicalStateRoot,
                genesisReceiptRoot,
                genesisCheckpoint,
                bondedSetCommitment,
                bondedSetVersion,
                registryAddress,
                vaultAddress,
                bondAddress,
                checkpointAddress
            )
        );
        if (identifier == bytes32(0)) revert InvalidManagerConfiguration();
        deploymentId = identifier;
        genesisFinalized = true;
        emit GenesisDeploymentFinalized(identifier, genesisCheckpoint, bondedSetCommitment, bondedSetVersion);
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
        _requireAddress(initialManifests[3], bytes4(keccak256("vault()")), initialManifests[2].component);
        _requireAddress(initialManifests[3], bytes4(keccak256("bondToken()")), usdlToken);
        _requireBytes32(initialManifests[3], bytes4(keccak256("assetId()")), usdlAssetId);
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

    function _requireBytes32(Preinstalls.ComponentManifest calldata manifest, bytes4 selector, bytes32 expected)
        private
        view
    {
        SafeCall.CallResult memory result =
            SafeCall.staticCall(manifest.component, abi.encodeWithSelector(selector), 50_000, 32);
        if (!result.success || result.returnDataSize != 32 || abi.decode(result.returnData, (bytes32)) != expected) {
            revert InvalidComponent(manifest.role, manifest.component);
        }
    }
}

interface IGenesisVault {
    function assetRegistry() external view returns (address);
    function guarantorBond() external view returns (address);
    function totalCustodied(bytes32 assetId) external view returns (uint256);
}

interface IGenesisBond {
    function vault() external view returns (address);
    function bondToken() external view returns (address);
    function assetId() external view returns (bytes32);
    function custodiedValue() external view returns (uint256);
    function membershipVersion() external view returns (uint64);
    function totalBonded() external view returns (uint256);
    function slashedBalance() external view returns (uint256);
    function genesisBondedSetCommitment() external view returns (bytes32);
    function genesisBondedSetVersion() external view returns (uint64);
}

interface IGenesisCheckpointRegistry {
    function protocolVersion() external view returns (uint16);
    function genesisManifestDigest() external view returns (bytes32);
    function genesisCanonicalStateRoot() external view returns (bytes32);
    function genesisReceiptRoot() external view returns (bytes32);
    function genesisCheckpointId() external view returns (bytes32);
    function derivedGenesisCheckpointId() external view returns (bytes32);
    function staticConfigHash() external view returns (bytes32);
}
