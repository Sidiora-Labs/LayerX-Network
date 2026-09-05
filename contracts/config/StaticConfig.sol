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
        bytes32 genesisManifestDigest;
        bytes32 genesisCanonicalStateRoot;
        bytes32 genesisReceiptRoot;
        address usdlToken;
        bytes32 usdlAssetId;
        uint8 usdlDecimals;
        uint8 usdlProtocolDecimals;
        uint128 usdlMinimumDeposit;
        uint128 usdlCustodyCap;
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
        validateForProtocol(config, actualChainId, Constants.PROTOCOL_VERSION);
    }

    function validateForProtocol(Config memory config, uint256 actualChainId, uint16 selectedProtocolVersion)
        internal
        pure
    {
        if (config.chainId != actualChainId) {
            revert StaticConfigWrongChain(config.chainId, actualChainId);
        }
        if (
            (selectedProtocolVersion != Constants.PROTOCOL_VERSION && selectedProtocolVersion != 3)
                || config.protocolVersion != selectedProtocolVersion
        ) revert InvalidStaticConfig();
        if (
            config.chainId == 0 || config.protocolVersion != selectedProtocolVersion || config.releaseVersion == 0
                || config.governanceTimelock == address(0) || config.emergencyCouncil == address(0)
                || config.governanceTimelock == config.emergencyCouncil || config.genesisManifestDigest == bytes32(0)
                || config.genesisCanonicalStateRoot == bytes32(0) || config.genesisReceiptRoot == bytes32(0)
                || config.usdlToken != Constants.USDL_TOKEN
                || config.genesisManifestDigest == config.genesisCanonicalStateRoot
                || config.genesisManifestDigest == config.genesisReceiptRoot
                || config.genesisCanonicalStateRoot == config.genesisReceiptRoot
                || config.usdlAssetId != Constants.USDL_ASSET_ID || config.usdlDecimals != Constants.USDL_TOKEN_DECIMALS
                || config.usdlProtocolDecimals != Constants.USDL_PROTOCOL_DECIMALS || config.usdlMinimumDeposit == 0
                || config.usdlCustodyCap < config.usdlMinimumDeposit || config.challengeWindow < 1 hours
                || config.checkpointLivenessBound < 1 hours || config.assetDefinitionsRoot == bytes32(0)
        ) {
            revert InvalidStaticConfig();
        }
        AssetDefinition[] memory assets = new AssetDefinition[](1);
        assets[0] = AssetDefinition({
            assetId: config.usdlAssetId,
            token: config.usdlToken,
            tokenDecimals: config.usdlDecimals,
            protocolDecimals: config.usdlProtocolDecimals,
            minimumDeposit: config.usdlMinimumDeposit,
            custodyCap: config.usdlCustodyCap
        });
        if (config.assetDefinitionsRoot != hashAssets(assets)) revert InvalidStaticConfig();
        Features.validate(config.enabledFeatures);
    }

    function hash(Config memory config, uint256 actualChainId) internal pure returns (bytes32) {
        return hashForProtocol(config, actualChainId, Constants.PROTOCOL_VERSION);
    }

    function hashForProtocol(Config memory config, uint256 actualChainId, uint16 selectedProtocolVersion)
        internal
        pure
        returns (bytes32)
    {
        validateForProtocol(config, actualChainId, selectedProtocolVersion);
        return keccak256(
            abi.encode(
                "LXP/Paxeer/static-config/v2",
                config.chainId,
                config.protocolVersion,
                config.releaseVersion,
                config.governanceTimelock,
                config.emergencyCouncil,
                config.genesisManifestDigest,
                config.genesisCanonicalStateRoot,
                config.genesisReceiptRoot,
                config.usdlToken,
                config.usdlAssetId,
                config.usdlDecimals,
                config.usdlProtocolDecimals,
                config.usdlMinimumDeposit,
                config.usdlCustodyCap,
                config.challengeWindow,
                config.checkpointLivenessBound,
                config.enabledFeatures,
                config.assetDefinitionsRoot
            )
        );
    }
}
