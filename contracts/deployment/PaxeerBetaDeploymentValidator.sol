// SPDX-License-Identifier: LicenseRef-Centra-ai-Protocol
pragma solidity ^0.8.24;

import {Constants} from "../libraries/Constants.sol";
import {SemverComp} from "../libraries/SemverComp.sol";
import {StaticConfig} from "../config/StaticConfig.sol";

library PaxeerBetaDeploymentValidator {
    uint256 internal constant PAXEER_EVM_CHAIN_ID = 125;
    uint256 internal constant GENESIS_DESCRIPTOR_BYTES = 105;
    uint256 internal constant REGISTRATION_REQUEST_BYTES = 73;

    error InvalidBetaDeploymentInput();
    error WrongPaxeerChain(uint256 actual);
    error InvalidUsdl();
    error InvalidGenesisArtifacts();
    error InvalidGuarantor(uint256 index);
    error UnfundedGuarantor(uint256 index);

    struct GenesisArtifacts {
        uint32 networkId;
        bytes32 manifestDigest;
        bytes32 canonicalStateRoot;
        bytes32 receiptRoot;
    }

    struct GuarantorInput {
        bytes32 guarantorId;
        address signer;
        address bondController;
        uint64 joinedEpoch;
        uint64 governanceSequence;
        uint256 bondAmount;
    }

    struct Input {
        string release;
        address bootstrapOperator;
        address finalProposer;
        address finalExecutor;
        address emergencyCouncil;
        uint64 timelockDelay;
        uint64 timelockGracePeriod;
        uint256 timelockMaximumCallValue;
        uint128 usdlMinimumDeposit;
        uint128 usdlCustodyCap;
        uint64 challengeWindow;
        uint64 checkpointLivenessBound;
        uint32 minimumBondBps;
        uint64 unbondingDelay;
        uint16 checkpointThresholdNumerator;
        uint16 checkpointThresholdDenominator;
        uint64 checkpointMaximumAge;
        uint64 checkpointFutureDrift;
        uint128 challengeBond;
        uint64 emergencyDelay;
        uint64 migrationDelay;
        uint64 migrationExpiry;
        uint64 migrationGasLimit;
        uint256 migrationMaximumCallValue;
        uint256 enabledFeatures;
    }

    function decodeAndCrossCheckGenesis(bytes calldata descriptor, bytes calldata registrationRequest)
        internal
        pure
        returns (GenesisArtifacts memory artifacts)
    {
        if (
            descriptor.length != GENESIS_DESCRIPTOR_BYTES || registrationRequest.length != REGISTRATION_REQUEST_BYTES
                || bytes4(descriptor[0:4]) != bytes4("LXGD") || uint8(descriptor[4]) != 1
                || bytes4(registrationRequest[0:4]) != bytes4("LXRR") || uint8(registrationRequest[4]) != 1
        ) revert InvalidGenesisArtifacts();
        artifacts.networkId = _u32(descriptor, 5);
        uint32 registrationNetwork = _u32(registrationRequest, 5);
        assembly ("memory-safe") {
            mstore(add(artifacts, 32), calldataload(add(descriptor.offset, 9)))
            mstore(add(artifacts, 64), calldataload(add(descriptor.offset, 41)))
            mstore(add(artifacts, 96), calldataload(add(descriptor.offset, 73)))
        }
        bytes32 registrationCanonical;
        bytes32 registrationReceipt;
        assembly ("memory-safe") {
            registrationCanonical := calldataload(add(registrationRequest.offset, 9))
            registrationReceipt := calldataload(add(registrationRequest.offset, 41))
        }
        if (
            artifacts.networkId == 0 || registrationNetwork != artifacts.networkId
                || artifacts.manifestDigest == bytes32(0) || artifacts.canonicalStateRoot == bytes32(0)
                || artifacts.receiptRoot == bytes32(0) || artifacts.manifestDigest == artifacts.canonicalStateRoot
                || artifacts.manifestDigest == artifacts.receiptRoot
                || artifacts.canonicalStateRoot == artifacts.receiptRoot
                || registrationCanonical != artifacts.canonicalStateRoot || registrationReceipt != artifacts.receiptRoot
        ) revert InvalidGenesisArtifacts();
    }

    function validateInput(Input memory input, GenesisArtifacts memory genesis)
        internal
        view
        returns (uint192 releaseVersion, StaticConfig.Config memory config)
    {
        return validateInputForProtocol(input, genesis, Constants.PROTOCOL_VERSION);
    }

    function validateInputForProtocol(
        Input memory input,
        GenesisArtifacts memory genesis,
        uint16 selectedProtocolVersion
    ) internal view returns (uint192 releaseVersion, StaticConfig.Config memory config) {
        if (selectedProtocolVersion != Constants.PROTOCOL_VERSION && selectedProtocolVersion != 3) {
            revert InvalidBetaDeploymentInput();
        }
        if (block.chainid != PAXEER_EVM_CHAIN_ID) revert WrongPaxeerChain(block.chainid);
        _validateUsdl();
        releaseVersion = SemverComp.parseRelease(input.release);
        if (
            releaseVersion == 0 || genesis.networkId == 0 || input.bootstrapOperator == address(0)
                || input.finalProposer == address(0) || input.finalExecutor == address(0)
                || input.emergencyCouncil == address(0) || input.bootstrapOperator == input.finalProposer
                || input.bootstrapOperator == input.finalExecutor || input.bootstrapOperator == input.emergencyCouncil
                || input.timelockDelay < 1 days || input.timelockGracePeriod < 1 days
                || input.timelockMaximumCallValue > 100 ether || input.usdlMinimumDeposit == 0
                || input.usdlCustodyCap < input.usdlMinimumDeposit || input.challengeWindow < 1 hours
                || input.checkpointLivenessBound < 1 hours || input.minimumBondBps == 0 || input.minimumBondBps > 10_000
                || input.unbondingDelay < 1 days || input.checkpointThresholdNumerator == 0
                || input.checkpointThresholdDenominator == 0
                || input.checkpointThresholdNumerator > input.checkpointThresholdDenominator
                || input.checkpointMaximumAge == 0 || input.checkpointFutureDrift == 0 || input.challengeBond == 0
                || input.emergencyDelay < 1 days || input.migrationDelay < 1 days || input.migrationExpiry < 1 days
                || input.migrationGasLimit == 0 || input.migrationMaximumCallValue > input.timelockMaximumCallValue
        ) revert InvalidBetaDeploymentInput();
        StaticConfig.AssetDefinition[] memory assets = new StaticConfig.AssetDefinition[](1);
        assets[0] = StaticConfig.AssetDefinition({
            assetId: Constants.USDL_ASSET_ID,
            token: Constants.USDL_TOKEN,
            tokenDecimals: Constants.USDL_TOKEN_DECIMALS,
            protocolDecimals: Constants.USDL_PROTOCOL_DECIMALS,
            minimumDeposit: input.usdlMinimumDeposit,
            custodyCap: input.usdlCustodyCap
        });
        config = StaticConfig.Config({
            chainId: PAXEER_EVM_CHAIN_ID,
            protocolVersion: selectedProtocolVersion,
            releaseVersion: releaseVersion,
            governanceTimelock: address(0),
            emergencyCouncil: input.emergencyCouncil,
            genesisManifestDigest: genesis.manifestDigest,
            genesisCanonicalStateRoot: genesis.canonicalStateRoot,
            genesisReceiptRoot: genesis.receiptRoot,
            usdlToken: Constants.USDL_TOKEN,
            usdlAssetId: Constants.USDL_ASSET_ID,
            usdlDecimals: Constants.USDL_TOKEN_DECIMALS,
            usdlProtocolDecimals: Constants.USDL_PROTOCOL_DECIMALS,
            usdlMinimumDeposit: input.usdlMinimumDeposit,
            usdlCustodyCap: input.usdlCustodyCap,
            challengeWindow: input.challengeWindow,
            checkpointLivenessBound: input.checkpointLivenessBound,
            enabledFeatures: input.enabledFeatures,
            assetDefinitionsRoot: StaticConfig.hashAssets(assets)
        });
    }

    function validateGuarantors(GuarantorInput[] memory guarantors, address guarantorBond) internal view {
        if (guarantors.length == 0 || guarantorBond == address(0)) revert InvalidBetaDeploymentInput();
        bytes32 previous;
        for (uint256 i = 0; i < guarantors.length; ++i) {
            GuarantorInput memory guarantor = guarantors[i];
            if (
                guarantor.guarantorId <= previous || guarantor.signer == address(0)
                    || guarantor.bondController == address(0) || guarantor.joinedEpoch == 0
                    || guarantor.governanceSequence == 0 || guarantor.bondAmount == 0
            ) revert InvalidGuarantor(i);
            for (uint256 j = 0; j < i; ++j) {
                if (
                    guarantors[j].signer == guarantor.signer || guarantors[j].bondController == guarantor.bondController
                ) revert InvalidGuarantor(i);
            }
            if (
                _tokenWord(Constants.USDL_TOKEN, 0x70a08231, guarantor.bondController, address(0))
                        < guarantor.bondAmount
                    || _tokenWord(Constants.USDL_TOKEN, 0xdd62ed3e, guarantor.bondController, guarantorBond)
                        < guarantor.bondAmount
            ) revert UnfundedGuarantor(i);
            previous = guarantor.guarantorId;
        }
    }

    function _validateUsdl() private view {
        if (Constants.USDL_TOKEN.code.length == 0) revert InvalidUsdl();
        (bool success, bytes memory data) = Constants.USDL_TOKEN.staticcall(abi.encodeWithSelector(0x313ce567));
        if (!success || data.length != 32 || abi.decode(data, (uint256)) != Constants.USDL_TOKEN_DECIMALS) {
            revert InvalidUsdl();
        }
    }

    function _tokenWord(address token, bytes4 selector, address first, address second)
        private
        view
        returns (uint256 value)
    {
        bytes memory callData = second == address(0)
            ? abi.encodeWithSelector(selector, first)
            : abi.encodeWithSelector(selector, first, second);
        (bool success, bytes memory data) = token.staticcall(callData);
        if (!success || data.length != 32) revert InvalidUsdl();
        value = abi.decode(data, (uint256));
    }

    function _u32(bytes calldata value, uint256 offset) private pure returns (uint32 result) {
        result = uint32(uint8(value[offset])) << 24 | uint32(uint8(value[offset + 1])) << 16
            | uint32(uint8(value[offset + 2])) << 8 | uint32(uint8(value[offset + 3]));
    }
}
