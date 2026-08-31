// SPDX-License-Identifier: LicenseRef-Centra-ai-Protocol
pragma solidity ^0.8.24;

import {Features} from "./Features.sol";
import {Constants} from "../libraries/Constants.sol";
import {DecimalsConverterHelper} from "../libraries/DecimalsConverterHelper.sol";

library StaticConfig {
    struct AssetDefinition {
        bytes32 assetId;
        address token;
        uint8 tokenDecimals;
        uint8 protocolDecimals;
        uint128 minimumDeposit;
        uint128 custodyCap;
    }

    struct Config {
        uint256 chainId;
        uint16 protocolVersion;
        uint192 releaseVersion;
        address governanceTimelock;
        address emergencyCouncil;
        bytes32 genesisReceiptRoot;
        uint64 challengeWindow;
        uint64 checkpointLivenessBound;
        uint256 enabledFeatures;
        bytes32 assetDefinitionsRoot;
    }

    error InvalidStaticConfig();
    error StaticConfigWrongChain(uint256 expected, uint256 actual);
    error InvalidAssetDefinition(uint256 index);
    error AssetDefinitionsNotOrdered(uint256 index);

    function hashAssets(AssetDefinition[] memory assets) internal pure returns (bytes32 root) {
        if (assets.length == 0) revert InvalidStaticConfig();
        root = keccak256("LXP/Paxeer/assets/v1");
        bytes32 previous;
        for (uint256 i = 0; i < assets.length; ++i) {
            AssetDefinition memory item = assets[i];
            if (
                item.assetId == bytes32(0) || item.token == address(0) || item.minimumDeposit == 0
                    || item.custodyCap < item.minimumDeposit
            ) {
                revert InvalidAssetDefinition(i);
            }
            if (i != 0 && item.assetId <= previous) {
                revert AssetDefinitionsNotOrdered(i);
            }
            DecimalsConverterHelper.scaleFactor(item.tokenDecimals);
            DecimalsConverterHelper.scaleFactor(item.protocolDecimals);
            root = keccak256(
                abi.encode(
                    root,
                    item.assetId,
                    item.token,
                    item.tokenDecimals,
                    item.protocolDecimals,
                    item.minimumDeposit,
                    item.custodyCap
                )
            );
            previous = item.assetId;
        }
    }

    function validate(Config memory config, uint256 actualChainId) internal pure {
        if (config.chainId != actualChainId) {
            revert StaticConfigWrongChain(config.chainId, actualChainId);
        }
        if (
            config.chainId == 0 || config.protocolVersion != Constants.PROTOCOL_VERSION || config.releaseVersion == 0
                || config.governanceTimelock == address(0) || config.emergencyCouncil == address(0)
                || config.governanceTimelock == config.emergencyCouncil || config.genesisReceiptRoot == bytes32(0)
                || config.challengeWindow < 1 hours || config.checkpointLivenessBound < 1 hours
                || config.assetDefinitionsRoot == bytes32(0)
        ) {
            revert InvalidStaticConfig();
        }
        Features.validate(config.enabledFeatures);
    }

    function hash(Config memory config, uint256 actualChainId) internal pure returns (bytes32) {
        validate(config, actualChainId);
        return keccak256(
            abi.encode(
                "LXP/Paxeer/static-config/v1",
                config.chainId,
                config.protocolVersion,
                config.releaseVersion,
                config.governanceTimelock,
                config.emergencyCouncil,
                config.genesisReceiptRoot,
                config.challengeWindow,
                config.checkpointLivenessBound,
                config.enabledFeatures,
                config.assetDefinitionsRoot
            )
        );
    }
}
