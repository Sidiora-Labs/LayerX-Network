// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

address constant ADDR_PRECOMPILE_ADDRESS = 0x0000000000000000000000000000000000001004;

IAddr constant ADDR_CONTRACT = IAddr(
    ADDR_PRECOMPILE_ADDRESS
);

interface IAddr {
    event LayerXBound(address indexed evm, bytes32 indexed didPublicKey, uint64 nonce);
    event LayerXUnbound(address indexed evm, bytes32 indexed didPublicKey, uint64 nonce);

    // Transactions
    // Binds msg.sender to did:layerx:<hex of didPublicKey>. The signature is the
    // DID key's strict Ed25519 signature over
    //   "LX:PAXEER-BIND:v1" || chain id (uint256 big-endian) || msg.sender (20 bytes)
    //   || layerXBindNonce(msg.sender) (uint64 big-endian).
    // One EVM address has at most one DID and one DID at most one EVM address.
    // Not callable through staticcall or delegatecall.
    function bindLayerX(bytes32 didPublicKey, bytes memory signature) external;

    // Removes msg.sender's binding and consumes a nonce.
    function unbindLayerX() external;

    // Queries
    function getPaxAddr(address addr) external view returns (string memory response);
    function getEvmAddr(string memory addr) external view returns (address response);

    // Reverts when addr has no LayerX binding.
    function getLayerXDid(address addr) external view returns (bytes32 didPublicKey, string memory did);
    // Reverts when the DID has no binding.
    function getEvmAddrByLayerX(bytes32 didPublicKey) external view returns (address evmAddr);
    // The nonce the next bindLayerX signature for addr must cover.
    function layerXBindNonce(address addr) external view returns (uint64 nonce);
    // Never reverts for a missing identity: paxAddr is empty without an
    // association, didPublicKey and layerxMainAccountId are zero without a
    // binding. layerxMainAccountId is the LayerX account id of
    // "agent:did:layerx:<hex>:main".
    function getUnifiedAccount(address addr) external view returns (
        address evm,
        string memory paxAddr,
        bytes32 didPublicKey,
        bytes32 layerxMainAccountId
    );
}
