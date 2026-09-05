// SPDX-License-Identifier: LicenseRef-Centra-ai-Protocol
pragma solidity ^0.8.24;

import {Constants} from "../libraries/Constants.sol";
import {StaticConfig} from "../config/StaticConfig.sol";
import {UUPSNotUpgradeable} from "../security/UUPSNotUpgradeable.sol";
import {ILayerXComponent} from "./Preinstalls.sol";
import {Predeploys} from "./Predeploys.sol";

contract Blueprint is UUPSNotUpgradeable {
    error ManagerOnly();
    error InvalidBlueprint();
    error DeploymentCollision(address predicted, bytes32 expectedCodeHash, bytes32 actualCodeHash);
    error DeploymentFailed(bytes32 role);
    error BlueprintSealed();

    address public immutable manager;
    uint16 public immutable protocolVersion;
    address public immutable governanceTimelock;
    address public immutable emergencyCouncil;
    bytes32 public immutable staticConfigHash;
    uint192 public immutable releaseVersion;
    mapping(bytes32 => address) public deploymentForRole;
    bytes32 public timelockInitCodeHash;
    bool public deploymentsSealed;

    event ComponentDeployed(
        bytes32 indexed role, address indexed component, bytes32 initCodeHash, bytes32 runtimeCodeHash, bytes32 salt
    );
    event DeploymentsSealed(bytes32 indexed staticConfigHash, uint192 release);

    constructor(address deploymentManager, StaticConfig.Config memory config) {
        if (deploymentManager == address(0) || config.governanceTimelock != address(0)) revert InvalidBlueprint();
        address predictedTimelock = _firstCreateAddress(address(this));
        config.governanceTimelock = predictedTimelock;
        bytes32 configHash = StaticConfig.hashForProtocol(config, block.chainid, config.protocolVersion);
        manager = deploymentManager;
        governanceTimelock = predictedTimelock;
        protocolVersion = config.protocolVersion;
        emergencyCouncil = config.emergencyCouncil;
        staticConfigHash = configHash;
        releaseVersion = config.releaseVersion;
    }

    function salt(bytes32 role, bytes32 initCodeHash) public view returns (bytes32) {
        if (role == bytes32(0) || initCodeHash == bytes32(0)) {
            revert InvalidBlueprint();
        }
        if (!Predeploys.isKnown(role) || role == Predeploys.TIMELOCK) revert InvalidBlueprint();
        return keccak256(
            abi.encode(
                Constants.DOMAIN_DEPLOYMENT,
                Constants.PROTOCOL_VERSION,
                block.chainid,
                address(this),
                staticConfigHash,
                releaseVersion,
                role,
                initCodeHash
            )
        );
    }

    function blueprintHash(bytes32 role, bytes32 initCodeHash, bytes32 expectedRuntimeCodeHash)
        external
        view
        returns (bytes32)
    {
        if (expectedRuntimeCodeHash == bytes32(0)) {
            revert InvalidBlueprint();
        }
        return keccak256(
            abi.encode(
                salt(role, initCodeHash), initCodeHash, expectedRuntimeCodeHash, staticConfigHash, releaseVersion
            )
        );
    }

    function predict(bytes32 role, bytes32 initCodeHash) public view returns (address) {
        bytes32 digest =
            keccak256(abi.encodePacked(bytes1(0xff), address(this), salt(role, initCodeHash), initCodeHash));
        return address(uint160(uint256(digest)));
    }

    function predictTimelock() public view returns (address) {
        return governanceTimelock;
    }

    function timelockBlueprintHash(bytes32 initCodeHash, bytes32 expectedRuntimeCodeHash)
        external
        view
        returns (bytes32)
    {
        if (initCodeHash == bytes32(0) || expectedRuntimeCodeHash == bytes32(0)) revert InvalidBlueprint();
        return keccak256(
            abi.encode(
                Constants.DOMAIN_DEPLOYMENT,
                Constants.PROTOCOL_VERSION,
                block.chainid,
                address(this),
                staticConfigHash,
                releaseVersion,
                Predeploys.TIMELOCK,
                initCodeHash,
                expectedRuntimeCodeHash,
                governanceTimelock
            )
        );
    }

    function deployTimelock(bytes calldata initCode, bytes32 expectedRuntimeCodeHash)
        external
        returns (address component)
    {
        if (msg.sender != manager) revert ManagerOnly();
        if (deploymentsSealed) revert BlueprintSealed();
        if (initCode.length == 0 || expectedRuntimeCodeHash == bytes32(0)) revert InvalidBlueprint();
        address predicted = governanceTimelock;
        bytes32 initCodeHash = keccak256(initCode);
        address existing = deploymentForRole[Predeploys.TIMELOCK];
        if (existing != address(0)) {
            if (
                existing != predicted || existing.codehash != expectedRuntimeCodeHash
                    || timelockInitCodeHash != initCodeHash
            ) {
                revert DeploymentCollision(existing, expectedRuntimeCodeHash, existing.codehash);
            }
            _validateTimelockAttestation(existing);
            return existing;
        }
        if (predicted.code.length != 0) {
            revert DeploymentCollision(predicted, expectedRuntimeCodeHash, predicted.codehash);
        }
        bytes memory creationCode = initCode;
        assembly ("memory-safe") {
            component := create(0, add(creationCode, 32), mload(creationCode))
        }
        if (component == address(0) || component != predicted) {
            revert DeploymentFailed(Predeploys.TIMELOCK);
        }
        if (component.codehash != expectedRuntimeCodeHash) {
            revert DeploymentCollision(component, expectedRuntimeCodeHash, component.codehash);
        }
        _validateTimelockAttestation(component);
        deploymentForRole[Predeploys.TIMELOCK] = component;
        timelockInitCodeHash = initCodeHash;
        emit ComponentDeployed(
            Predeploys.TIMELOCK, component, initCodeHash, expectedRuntimeCodeHash, bytes32(uint256(1))
        );
    }

    function deploy(bytes32 role, bytes calldata initCode, bytes32 expectedRuntimeCodeHash)
        external
        returns (address component)
    {
        if (msg.sender != manager) revert ManagerOnly();
        if (deploymentsSealed) revert BlueprintSealed();
        if (role == Predeploys.TIMELOCK || deploymentForRole[Predeploys.TIMELOCK] != governanceTimelock) {
            revert InvalidBlueprint();
        }
        if (initCode.length == 0 || expectedRuntimeCodeHash == bytes32(0)) {
            revert InvalidBlueprint();
        }
        bytes32 initCodeHash = keccak256(initCode);
        bytes32 deploymentSalt = salt(role, initCodeHash);
        address predicted = predict(role, initCodeHash);
        address existing = deploymentForRole[role];
        if (existing != address(0) && existing != predicted) {
            revert DeploymentCollision(existing, expectedRuntimeCodeHash, existing.codehash);
        }
        if (predicted.code.length != 0) {
            if (predicted.codehash != expectedRuntimeCodeHash) {
                revert DeploymentCollision(predicted, expectedRuntimeCodeHash, predicted.codehash);
            }
            deploymentForRole[role] = predicted;
            return predicted;
        }
        bytes memory creationCode = initCode;
        assembly ("memory-safe") {
            component := create2(0, add(creationCode, 32), mload(creationCode), deploymentSalt)
        }
        if (component == address(0) || component != predicted) {
            revert DeploymentFailed(role);
        }
        if (component.codehash != expectedRuntimeCodeHash) {
            revert DeploymentCollision(component, expectedRuntimeCodeHash, component.codehash);
        }
        deploymentForRole[role] = component;
        emit ComponentDeployed(role, component, initCodeHash, expectedRuntimeCodeHash, deploymentSalt);
    }

    function seal() external {
        if (msg.sender != manager) revert ManagerOnly();
        if (deploymentsSealed) revert BlueprintSealed();
        for (uint256 i = 0; i < Predeploys.COUNT; ++i) {
            bytes32 role = Predeploys.roleAt(i);
            _validateComponentAttestation(deploymentForRole[role], role);
        }
        address managerComponent = deploymentForRole[Predeploys.CONTRACTS_MANAGER];
        IManagerSealAttestation managerAttestation = IManagerSealAttestation(managerComponent);
        try managerAttestation.governanceTimelock() returns (address governance) {
            if (governance != governanceTimelock) revert InvalidBlueprint();
        } catch {
            revert InvalidBlueprint();
        }
        try managerAttestation.emergencyCouncil() returns (address emergency) {
            if (emergency != emergencyCouncil) revert InvalidBlueprint();
        } catch {
            revert InvalidBlueprint();
        }
        try managerAttestation.initialized() returns (bool initialized) {
            if (!initialized) revert InvalidBlueprint();
        } catch {
            revert InvalidBlueprint();
        }
        for (uint256 i = 0; i < Predeploys.COUNT; ++i) {
            bytes32 role = Predeploys.roleAt(i);
            try managerAttestation.componentForRole(role) returns (address component) {
                if (component != deploymentForRole[role]) revert InvalidBlueprint();
            } catch {
                revert InvalidBlueprint();
            }
        }
        try managerAttestation.genesisFinalized() returns (bool finalized) {
            if (!finalized) revert InvalidBlueprint();
        } catch {
            revert InvalidBlueprint();
        }
        try managerAttestation.deploymentId() returns (bytes32 identifier) {
            if (identifier == bytes32(0)) revert InvalidBlueprint();
        } catch {
            revert InvalidBlueprint();
        }
        deploymentsSealed = true;
        emit DeploymentsSealed(staticConfigHash, releaseVersion);
    }

    function _firstCreateAddress(address deployer) private pure returns (address) {
        return address(uint160(uint256(keccak256(abi.encodePacked(hex"d694", deployer, hex"01")))));
    }

    function _validateTimelockAttestation(address component) private view {
        _validateComponentAttestation(component, Predeploys.TIMELOCK);
    }

    function _validateComponentAttestation(address component, bytes32 expectedRole) private view {
        if (component == address(0) || component.code.length == 0) revert InvalidBlueprint();
        try ILayerXComponent(component).componentRole() returns (bytes32 role) {
            if (role != expectedRole) revert InvalidBlueprint();
        } catch {
            revert InvalidBlueprint();
        }
        try ILayerXComponent(component).staticConfigHash() returns (bytes32 configHash) {
            if (configHash != staticConfigHash) revert InvalidBlueprint();
        } catch {
            revert InvalidBlueprint();
        }
        try ILayerXComponent(component).releaseVersion() returns (uint192 release) {
            if (release != releaseVersion) revert InvalidBlueprint();
        } catch {
            revert InvalidBlueprint();
        }
        try ILayerXComponent(component).storageLayoutVersion() returns (uint16 storageLayout) {
            if (storageLayout != Constants.STORAGE_LAYOUT_VERSION) revert InvalidBlueprint();
        } catch {
            revert InvalidBlueprint();
        }
    }
}

interface IManagerSealAttestation {
    function governanceTimelock() external view returns (address);
    function emergencyCouncil() external view returns (address);
    function initialized() external view returns (bool);
    function genesisFinalized() external view returns (bool);
    function deploymentId() external view returns (bytes32);
    function componentForRole(bytes32 role) external view returns (address);
}
