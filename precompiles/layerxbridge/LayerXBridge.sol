// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

address constant LAYERX_BRIDGE_PRECOMPILE_ADDRESS = 0x0000000000000000000000000000000000001016;

ILayerXBridge constant LAYERX_BRIDGE_CONTRACT = ILayerXBridge(LAYERX_BRIDGE_PRECOMPILE_ADDRESS);

/// Attested bridge between Paxeer and registered external chains. Governance
/// registers chains (vault, finality depth), sets the attestor set and
/// threshold, sets per-asset caps and pauses. Genesis registers nothing, so
/// the bridge is dormant until governance brings a chain up.
///
/// bridgeIn mints the bridged denom of (chain, asset) to the Paxeer address in
/// the low 20 bytes of recipient against at least threshold attestor
/// signatures over the PaxeerXVault deposit digest
///   keccak256(abi.encodePacked(bytes20("PAXEERX_BRIDGE_IN_V1"), uint256(chain), vault, txHash, logIndex,
///                              recipient, asset, amount))
/// Each signature is 65 bytes r || s || v over the raw digest, v 27 or 28, low
/// s, ordered by strictly ascending signer. The remote event (chain, txHash,
/// logIndex) is bridged once. modules/layerxbridge/ATTESTATION.md is the
/// byte-exact specification.
///
/// bridgeOut burns the caller's bridged denom and emits BridgeOut with the
/// chain's next nonce; attestors sign the vault's outbound digest over it.
///
/// Gas = 3000 + 16 * len(calldata after the selector) + 3000 * signatures
///     + 5000 * writes
/// signatures: signatures.length for bridgeIn, 0 otherwise. writes: 8 for
/// bridgeIn, 6 for bridgeOut, 0 for views.
interface ILayerXBridge {
    event BridgeIn(
        uint64 indexed chain,
        bytes32 indexed txHash,
        address indexed recipient,
        uint64 logIndex,
        address asset,
        uint256 amount,
        string denom
    );

    event BridgeOut(uint64 indexed chain, address indexed asset, uint256 amount, address recipient, uint64 indexed nonce);

    function bridgeIn(
        uint64 chain,
        address vault,
        bytes32 txHash,
        uint64 logIndex,
        bytes32 recipient,
        address asset,
        uint256 amount,
        bytes[] calldata signatures
    ) external returns (string memory denom);

    function bridgeOut(uint64 chain, address asset, uint256 amount, address recipient)
        external
        returns (uint64 nonce);

    function getChain(uint64 chain)
        external
        view
        returns (bool registered, address vault, uint64 finalityDepth, bool enabled);

    function getAttestors() external view returns (address[] memory signers, uint256[] memory bonds, uint32 threshold);

    function getCap(uint64 chain, address asset)
        external
        view
        returns (string memory denom, uint256 maxInFlight, uint256 maxPerTx, uint256 inFlight);

    function isPaused() external view returns (bool);

    function isNullified(uint64 chain, bytes32 txHash, uint64 logIndex) external view returns (bool);
}
