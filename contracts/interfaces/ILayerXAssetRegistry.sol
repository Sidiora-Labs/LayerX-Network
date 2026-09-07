// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

interface ILayerXAssetRegistry {
    struct AssetConfig {
        address token;
        uint8 decimals;
        uint128 minimumDeposit;
        uint128 custodyCap;
        bool enabled;
        bool paused;
    }

    function asset(bytes32 assetId) external view returns (AssetConfig memory);
}
