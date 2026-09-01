// SPDX-License-Identifier: LicenseRef-Centra-ai-Protocol
pragma solidity ^0.8.24;

library Constants {
    uint16 internal constant PROTOCOL_VERSION = 2;
    uint16 internal constant STORAGE_LAYOUT_VERSION = 1;
    uint8 internal constant MAX_TOKEN_DECIMALS = 77;
    uint8 internal constant MAX_CUSTODY_TOKEN_DECIMALS = 36;
    uint16 internal constant MAX_MERKLE_DEPTH = 256;
    uint32 internal constant MAX_CANONICAL_BYTES = 1_048_576;
    uint32 internal constant MAX_MIGRATION_CALLDATA = 65_536;
    uint32 internal constant MAX_RETURN_DATA = 4_096;
    uint8 internal constant ALL_AVAILABILITY_CLASSES = 0x1f;
    address internal constant USDL_TOKEN = 0x85FcD13735F4309833A503EE804ea32395851479;
    bytes32 internal constant USDL_ASSET_ID = keccak256("USDL");
    uint8 internal constant USDL_TOKEN_DECIMALS = 6;
    uint8 internal constant USDL_PROTOCOL_DECIMALS = 18;

    uint256 internal constant SECP256K1_ORDER = 0xfffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141;
    uint256 internal constant SECP256K1_HALF_ORDER = 0x7fffffffffffffffffffffffffffffff5d576e7357a4501ddfe92f46681b20a0;

    bytes32 internal constant DOMAIN_CANONICAL_BYTES = keccak256("LXP/Paxeer/canonical-bytes/v1");
    bytes32 internal constant DOMAIN_SIGNATURE = keccak256("LXP/Paxeer/signature/v1");
    bytes32 internal constant DOMAIN_MERKLE_LEAF = keccak256("LXP/Paxeer/merkle-leaf/v1");
    bytes32 internal constant DOMAIN_MERKLE_NODE = keccak256("LXP/Paxeer/merkle-node/v1");
    bytes32 internal constant DOMAIN_DEPLOYMENT = keccak256("LXP/Paxeer/deployment/v1");
    bytes32 internal constant DOMAIN_MIGRATION = keccak256("LXP/Paxeer/migration/v1");
    bytes32 internal constant DOMAIN_MESSAGE = keccak256("LXP/Paxeer/message/v1");
    bytes32 internal constant DOMAIN_ERROR = keccak256("LXP/Paxeer/error/v1");
}
