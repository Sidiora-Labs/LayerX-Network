// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

library Features {
    uint256 internal constant ERC20_CUSTODY = 1 << 0;
    uint256 internal constant CHECKPOINT_CHALLENGES = 1 << 1;
    uint256 internal constant WITHDRAWAL_CLAIMS = 1 << 2;
    uint256 internal constant EMERGENCY_EXIT = 1 << 3;
    uint256 internal constant RESERVE_RECONCILIATION = 1 << 4;
    uint256 internal constant TRANSIENT_STORAGE = 1 << 5;

    uint256 internal constant SUPPORTED_MASK = ERC20_CUSTODY | CHECKPOINT_CHALLENGES | WITHDRAWAL_CLAIMS
        | EMERGENCY_EXIT | RESERVE_RECONCILIATION | TRANSIENT_STORAGE;

    uint256 internal constant ARBITRUM_ADDRESS_ALIASING = 1 << 248;
    uint256 internal constant ARBITRUM_PRECOMPILES = 1 << 249;
    uint256 internal constant OP_STACK_PREDEPLOYS = 1 << 250;
    uint256 internal constant FOREIGN_GAS_TOKEN_HOOKS = 1 << 251;
    uint256 internal constant FOREIGN_MASK =
        ARBITRUM_ADDRESS_ALIASING | ARBITRUM_PRECOMPILES | OP_STACK_PREDEPLOYS | FOREIGN_GAS_TOKEN_HOOKS;

    error UnsupportedFeature(uint256 bits);
    error ForeignChainFacility(uint256 bits);
    error RequiredFeatureDisabled(uint256 feature);

    function validate(uint256 enabled) internal pure {
        uint256 foreign = enabled & FOREIGN_MASK;
        if (foreign != 0) revert ForeignChainFacility(foreign);
        uint256 unknown = enabled & ~(SUPPORTED_MASK | FOREIGN_MASK);
        if (unknown != 0) revert UnsupportedFeature(unknown);
    }

    function requireEnabled(uint256 enabled, uint256 feature) internal pure {
        validate(enabled);
        if (feature == 0 || feature & (feature - 1) != 0 || enabled & feature == 0) {
            revert RequiredFeatureDisabled(feature);
        }
    }

    function isEnabled(uint256 enabled, uint256 feature) internal pure returns (bool) {
        validate(enabled);
        return feature != 0 && feature & (feature - 1) == 0 && enabled & feature != 0;
    }
}
