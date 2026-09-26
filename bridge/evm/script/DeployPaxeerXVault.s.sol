// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.30;

import {Script} from "forge-std/Script.sol";

import {PaxeerXVault} from "../src/PaxeerXVault.sol";

/// @notice Deploys one PaxeerXVault and registers the assets the chain
/// configuration declares. Every value arrives from the environment
/// bridge/deploy/deploy-evm-chain.sh exports out of that configuration - the
/// owner the deployment is handed to, the shared attestor set, the threshold,
/// and one per-transaction and one total cap per asset - so the same bytecode
/// deploys to every EVM chain of the bridge and no address, cap, threshold or
/// chain id is compiled in.
///
/// The deployer owns the vault for the length of the run only: the constructor
/// records the attestor set the configuration names, the run sets every cap,
/// and ownership is then handed to the configured owner. Ownership moves in two
/// steps, so the configured owner accepts it before it is the owner.
contract DeployPaxeerXVault is Script {
    /// @dev The asset id of a chain's native coin, deposited through
    /// depositNative and paid out by the native release path.
    address private constant NATIVE_ASSET = address(0);

    error ZeroOwner();
    error NoAttestors();
    error ZeroThreshold();
    error ThresholdAboveAttestors(uint256 threshold, uint256 attestors);
    error NoAssets();
    error CapCountMismatch(uint256 assets, uint256 perTxCaps, uint256 totalCaps);
    error NativeAssetNotFirst(address first);
    error ZeroCap(address asset, uint256 perTx, uint256 total);

    function run() external returns (PaxeerXVault vault) {
        address owner = vm.envAddress("PAXEER_BRIDGE_VAULT_OWNER");
        address[] memory attestors = vm.envAddress("PAXEER_BRIDGE_VAULT_ATTESTORS", ",");
        uint256 threshold = vm.envUint("PAXEER_BRIDGE_VAULT_THRESHOLD");
        address[] memory assets = vm.envAddress("PAXEER_BRIDGE_VAULT_ASSETS", ",");
        uint256[] memory perTxCaps = vm.envUint("PAXEER_BRIDGE_VAULT_PER_TX_CAPS", ",");
        uint256[] memory totalCaps = vm.envUint("PAXEER_BRIDGE_VAULT_TOTAL_CAPS", ",");

        if (owner == address(0)) revert ZeroOwner();
        if (attestors.length == 0) revert NoAttestors();
        if (threshold == 0) revert ZeroThreshold();
        if (threshold > attestors.length) revert ThresholdAboveAttestors(threshold, attestors.length);
        if (assets.length == 0) revert NoAssets();
        if (assets.length != perTxCaps.length || assets.length != totalCaps.length) {
            revert CapCountMismatch(assets.length, perTxCaps.length, totalCaps.length);
        }
        if (assets[0] != NATIVE_ASSET) revert NativeAssetNotFirst(assets[0]);
        for (uint256 i = 0; i < assets.length; ++i) {
            if (perTxCaps[i] == 0 || totalCaps[i] == 0) {
                revert ZeroCap(assets[i], perTxCaps[i], totalCaps[i]);
            }
        }

        vm.startBroadcast();
        vault = new PaxeerXVault(msg.sender, attestors, threshold);
        for (uint256 i = 0; i < assets.length; ++i) {
            vault.setCap(assets[i], perTxCaps[i], totalCaps[i]);
        }
        vault.transferOwnership(owner);
        vm.stopBroadcast();
    }
}
