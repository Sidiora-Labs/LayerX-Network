// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {Predeploys} from "./Predeploys.sol";
import {SafeCall} from "../libraries/SafeCall.sol";

interface ILayerXComponent {
    function componentRole() external view returns (bytes32);
    function staticConfigHash() external view returns (bytes32);
    function releaseVersion() external view returns (uint192);
    function storageLayoutVersion() external view returns (uint16);
}

library Preinstalls {
    struct ComponentManifest {
        bytes32 role;
        address component;
        bytes4 interfaceId;
        bytes32 runtimeCodeHash;
        bytes32 configHash;
        uint192 release;
        uint16 storageLayout;
    }

    error InvalidComponentManifest(bytes32 role, address component);
    error IncompleteComponentManifest(uint256 expected, uint256 actual);
    error DuplicateComponentAddress(address component);
    error ComponentAttestationFailed(address component, bytes4 selector);

    function interfaceId() internal pure returns (bytes4) {
        return type(ILayerXComponent).interfaceId;
    }

    function hash(ComponentManifest memory manifest) internal pure returns (bytes32) {
        return keccak256(
            abi.encode(
                "LXP/Paxeer/preinstall/v1",
                manifest.role,
                manifest.component,
                manifest.interfaceId,
                manifest.runtimeCodeHash,
                manifest.configHash,
                manifest.release,
                manifest.storageLayout
            )
        );
    }

    function validate(ComponentManifest memory manifest) internal view {
        if (
            !Predeploys.isKnown(manifest.role) || manifest.component == address(0)
                || manifest.interfaceId != interfaceId() || manifest.runtimeCodeHash == bytes32(0)
                || manifest.component.codehash != manifest.runtimeCodeHash || manifest.configHash == bytes32(0)
                || manifest.release == 0 || manifest.storageLayout == 0
        ) {
            revert InvalidComponentManifest(manifest.role, manifest.component);
        }
        if (
            _readWord(manifest.component, ILayerXComponent.componentRole.selector) != manifest.role
                || _readWord(manifest.component, ILayerXComponent.staticConfigHash.selector) != manifest.configHash
                || uint256(_readWord(manifest.component, ILayerXComponent.releaseVersion.selector)) != manifest.release
                || uint256(_readWord(manifest.component, ILayerXComponent.storageLayoutVersion.selector))
                    != manifest.storageLayout
        ) {
            revert InvalidComponentManifest(manifest.role, manifest.component);
        }
    }

    function validateComplete(ComponentManifest[] memory manifests) internal view returns (bytes32 manifestRoot) {
        if (manifests.length != Predeploys.COUNT) {
            revert IncompleteComponentManifest(Predeploys.COUNT, manifests.length);
        }
        manifestRoot = keccak256("LXP/Paxeer/preinstall-manifest/v1");
        for (uint256 i = 0; i < manifests.length; ++i) {
            if (manifests[i].role != Predeploys.roleAt(i)) {
                revert InvalidComponentManifest(manifests[i].role, manifests[i].component);
            }
            for (uint256 j = 0; j < i; ++j) {
                if (manifests[j].component == manifests[i].component) {
                    revert DuplicateComponentAddress(manifests[i].component);
                }
            }
            validate(manifests[i]);
            manifestRoot = keccak256(abi.encode(manifestRoot, i, hash(manifests[i])));
        }
    }

    function _readWord(address component, bytes4 selector) private view returns (bytes32 value) {
        SafeCall.CallResult memory result = SafeCall.staticCall(component, abi.encodeWithSelector(selector), 50_000, 32);
        if (!result.success || result.returnDataSize != 32) {
            revert ComponentAttestationFailed(component, selector);
        }
        bytes memory data = result.returnData;
        assembly ("memory-safe") { value := mload(add(data, 32)) }
    }
}
