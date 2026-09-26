// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.30;

/// @notice Byte layouts of the digests attestors sign for the PaxeerX bridge.
/// Both are raw keccak256 digests over packed big-endian fields, with no
/// EIP-191 prefix and no EIP-712 structure. See ATTESTATION.md.
library BridgeAttestation {
    /// @dev Ethereum -> Paxeer: attests a BridgeDeposit log on the vault.
    bytes20 internal constant DOMAIN_IN = "PAXEERX_BRIDGE_IN_V1";
    /// @dev Paxeer -> Ethereum: attests a Paxeer burn authorising a release.
    bytes21 internal constant DOMAIN_OUT = "PAXEERX_BRIDGE_OUT_V1";

    uint256 internal constant IN_PREIMAGE_LENGTH = 20 + 32 + 20 + 32 + 8 + 32 + 20 + 32;
    uint256 internal constant OUT_PREIMAGE_LENGTH = 21 + 32 + 20 + 32 + 8 + 20 + 20 + 32;

    function outboundPreimage(
        uint256 chainId,
        address vault,
        bytes32 paxeerTxHash,
        uint64 paxeerNonce,
        address recipient,
        address asset,
        uint256 amount
    ) internal pure returns (bytes memory) {
        return abi.encodePacked(DOMAIN_OUT, chainId, vault, paxeerTxHash, paxeerNonce, recipient, asset, amount);
    }

    function outboundDigest(
        uint256 chainId,
        address vault,
        bytes32 paxeerTxHash,
        uint64 paxeerNonce,
        address recipient,
        address asset,
        uint256 amount
    ) internal pure returns (bytes32) {
        return keccak256(outboundPreimage(chainId, vault, paxeerTxHash, paxeerNonce, recipient, asset, amount));
    }

    function inboundPreimage(
        uint256 chainId,
        address vault,
        bytes32 txHash,
        uint64 logIndex,
        bytes32 paxeerRecipient,
        address asset,
        uint256 amount
    ) internal pure returns (bytes memory) {
        return abi.encodePacked(DOMAIN_IN, chainId, vault, txHash, logIndex, paxeerRecipient, asset, amount);
    }

    function inboundDigest(
        uint256 chainId,
        address vault,
        bytes32 txHash,
        uint64 logIndex,
        bytes32 paxeerRecipient,
        address asset,
        uint256 amount
    ) internal pure returns (bytes32) {
        return keccak256(inboundPreimage(chainId, vault, txHash, logIndex, paxeerRecipient, asset, amount));
    }
}
