// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {ILayerXComponent} from "../deployment/Preinstalls.sol";
import {Predeploys} from "../deployment/Predeploys.sol";
import {Constants} from "../libraries/Constants.sol";
import {UUPSNotUpgradeable} from "./UUPSNotUpgradeable.sol";

abstract contract LayerXComponent is ILayerXComponent, UUPSNotUpgradeable {
    error InvalidComponentAttestation();

    bytes32 public immutable override componentRole;
    bytes32 public immutable override staticConfigHash;
    uint192 public immutable override releaseVersion;
    uint16 public constant override storageLayoutVersion = Constants.STORAGE_LAYOUT_VERSION;

    constructor(bytes32 role, bytes32 configHash, uint192 release) {
        if (!Predeploys.isKnown(role) || configHash == bytes32(0) || release == 0) {
            revert InvalidComponentAttestation();
        }
        componentRole = role;
        staticConfigHash = configHash;
        releaseVersion = release;
    }
}
