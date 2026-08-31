// SPDX-License-Identifier: LicenseRef-Centra-ai-Protocol
pragma solidity ^0.8.24;

interface IBlueprintPredictor {
    function predict(bytes32 role, bytes32 initCodeHash) external view returns (address);
    function predictTimelock() external view returns (address);
}

library Predeploys {
    uint256 internal constant COUNT = 13;

    bytes32 internal constant TIMELOCK = keccak256("LXP_ROLE_TIMELOCK");
    bytes32 internal constant ASSET_REGISTRY = keccak256("LXP_ROLE_ASSET_REGISTRY");
    bytes32 internal constant VAULT = keccak256("LXP_ROLE_VAULT");
    bytes32 internal constant GUARANTOR_BOND = keccak256("LXP_ROLE_GUARANTOR_BOND");
    bytes32 internal constant CHECKPOINT_REGISTRY = keccak256("LXP_ROLE_CHECKPOINT_REGISTRY");
    bytes32 internal constant CHALLENGE_MANAGER = keccak256("LXP_ROLE_CHALLENGE_MANAGER");
    bytes32 internal constant NULLIFIER_REGISTRY = keccak256("LXP_ROLE_NULLIFIER_REGISTRY");
    bytes32 internal constant WITHDRAWAL_CLAIMS = keccak256("LXP_ROLE_WITHDRAWAL_CLAIMS");
    bytes32 internal constant EMERGENCY_EXIT = keccak256("LXP_ROLE_EMERGENCY_EXIT");
    bytes32 internal constant RESERVE_RECONCILER = keccak256("LXP_ROLE_RESERVE_RECONCILER");
    bytes32 internal constant CONTRACTS_MANAGER = keccak256("LXP_ROLE_CONTRACTS_MANAGER");
    bytes32 internal constant MANAGER_MIGRATOR = keccak256("LXP_ROLE_MANAGER_MIGRATOR");
    bytes32 internal constant CUSTODY_TOPOLOGY = keccak256("LXP_ROLE_CUSTODY_TOPOLOGY");

    error UnknownPredeployRole(bytes32 role);
    error PredeployIndexOutOfRange(uint256 index);

    function roleAt(uint256 index) internal pure returns (bytes32) {
        if (index == 0) return TIMELOCK;
        if (index == 1) return ASSET_REGISTRY;
        if (index == 2) return VAULT;
        if (index == 3) return GUARANTOR_BOND;
        if (index == 4) return CHECKPOINT_REGISTRY;
        if (index == 5) return CHALLENGE_MANAGER;
        if (index == 6) return NULLIFIER_REGISTRY;
        if (index == 7) return WITHDRAWAL_CLAIMS;
        if (index == 8) return EMERGENCY_EXIT;
        if (index == 9) return RESERVE_RECONCILER;
        if (index == 10) return CONTRACTS_MANAGER;
        if (index == 11) return MANAGER_MIGRATOR;
        if (index == 12) return CUSTODY_TOPOLOGY;
        revert PredeployIndexOutOfRange(index);
    }

    function indexOf(bytes32 role) internal pure returns (uint256) {
        for (uint256 i = 0; i < COUNT; ++i) {
            if (roleAt(i) == role) return i;
        }
        revert UnknownPredeployRole(role);
    }

    function isKnown(bytes32 role) internal pure returns (bool) {
        for (uint256 i = 0; i < COUNT; ++i) {
            if (roleAt(i) == role) return true;
        }
        return false;
    }

    function predictedAddress(IBlueprintPredictor blueprint, bytes32 role, bytes32 initCodeHash)
        internal
        view
        returns (address)
    {
        if (!isKnown(role)) revert UnknownPredeployRole(role);
        if (role == TIMELOCK) return blueprint.predictTimelock();
        return blueprint.predict(role, initCodeHash);
    }
}
