// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {ILayerXAssetRegistry} from "../interfaces/ILayerXAssetRegistry.sol";
import {Governed} from "../security/Governed.sol";
import {LayerXComponent} from "../security/LayerXComponent.sol";
import {Predeploys} from "../deployment/Predeploys.sol";
import {Constants} from "../libraries/Constants.sol";

contract AssetRegistry is ILayerXAssetRegistry, Governed, LayerXComponent {
    error InvalidAsset();
    error AssetAlreadyRegistered();
    error AssetNotRegistered();

    mapping(bytes32 => AssetConfig) private assets;

    event AssetRegistered(
        bytes32 indexed assetId, address indexed token, uint8 decimals, uint128 minimumDeposit, uint128 custodyCap
    );
    event AssetRiskUpdated(bytes32 indexed assetId, uint128 minimumDeposit, uint128 custodyCap, bool enabled);
    event AssetPauseSet(bytes32 indexed assetId, bool paused);

    constructor(
        address governanceTimelock,
        address emergencyAuthority,
        bytes32 componentConfigHash,
        uint192 componentRelease
    )
        Governed(governanceTimelock, emergencyAuthority)
        LayerXComponent(Predeploys.ASSET_REGISTRY, componentConfigHash, componentRelease)
    {}

    function asset(bytes32 assetId) external view returns (AssetConfig memory) {
        AssetConfig memory config = assets[assetId];
        if (config.token == address(0)) revert AssetNotRegistered();
        return config;
    }

    function registerAsset(bytes32 assetId, address token, uint8 decimals, uint128 minimumDeposit, uint128 custodyCap)
        external
        onlyGovernance
    {
        if (
            assetId == bytes32(0) || token.code.length == 0 || decimals > Constants.MAX_CUSTODY_TOKEN_DECIMALS
                || minimumDeposit == 0 || custodyCap < minimumDeposit
        ) {
            revert InvalidAsset();
        }
        if (assets[assetId].token != address(0)) {
            revert AssetAlreadyRegistered();
        }
        assets[assetId] = AssetConfig({
            token: token,
            decimals: decimals,
            minimumDeposit: minimumDeposit,
            custodyCap: custodyCap,
            enabled: true,
            paused: false
        });
        emit AssetRegistered(assetId, token, decimals, minimumDeposit, custodyCap);
    }

    function updateRisk(bytes32 assetId, uint128 minimumDeposit, uint128 custodyCap, bool enabled)
        external
        onlyGovernance
    {
        AssetConfig storage config = assets[assetId];
        if (config.token == address(0) || minimumDeposit == 0 || custodyCap < minimumDeposit) revert InvalidAsset();
        config.minimumDeposit = minimumDeposit;
        config.custodyCap = custodyCap;
        config.enabled = enabled;
        emit AssetRiskUpdated(assetId, minimumDeposit, custodyCap, enabled);
    }

    function emergencyPause(bytes32 assetId) external onlyEmergencyCouncil {
        AssetConfig storage config = assets[assetId];
        if (config.token == address(0)) revert AssetNotRegistered();
        config.paused = true;
        emit AssetPauseSet(assetId, true);
    }

    function governanceUnpause(bytes32 assetId) external onlyGovernance {
        AssetConfig storage config = assets[assetId];
        if (config.token == address(0)) revert AssetNotRegistered();
        config.paused = false;
        emit AssetPauseSet(assetId, false);
    }
}
