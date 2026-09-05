// SPDX-License-Identifier: LicenseRef-Centra-ai-Protocol
pragma solidity ^0.8.24;

import {LayerXTimelockCore} from "./LayerXTimelock.sol";

contract LayerXBetaTimelock is LayerXTimelockCore {
    constructor(
        uint64 minimumDelay,
        uint64 executionGracePeriod,
        address initialProposer,
        address initialExecutor,
        address initialGuardian,
        uint256 callValueLimit,
        bytes32 componentConfigHash,
        uint192 componentRelease
    )
        LayerXTimelockCore(
            minimumDelay,
            executionGracePeriod,
            initialProposer,
            initialExecutor,
            initialGuardian,
            callValueLimit,
            componentConfigHash,
            componentRelease,
            0
        )
    {
        if (block.chainid != 125 || minimumDelay != 0) revert InvalidOperation();
    }
}
