// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {CheckpointRegistry} from "./CheckpointRegistry.sol";
import {GuarantorBond} from "./GuarantorBond.sol";
import {LayerXVault} from "./custody/LayerXVault.sol";
import {CheckpointChallengeManager} from "./challenge/CheckpointChallengeManager.sol";
import {WithdrawalNullifierRegistry} from "./storage/WithdrawalNullifierRegistry.sol";
import {WithdrawalClaims} from "./WithdrawalClaims.sol";
import {LayerXComponent} from "./security/LayerXComponent.sol";
import {Predeploys} from "./deployment/Predeploys.sol";

contract LayerXCustody is LayerXComponent {
    error InvalidCustodyTopology();

    CheckpointRegistry public immutable checkpointRegistry;
    GuarantorBond public immutable guarantorBond;
    LayerXVault public immutable vault;
    CheckpointChallengeManager public immutable challengeManager;
    WithdrawalNullifierRegistry public immutable withdrawalNullifiers;
    WithdrawalClaims public immutable withdrawalClaims;
    bytes32 public immutable topologyHash;

    constructor(
        CheckpointRegistry registry,
        GuarantorBond bond,
        LayerXVault custodyVault,
        CheckpointChallengeManager challenges,
        WithdrawalNullifierRegistry nullifiers,
        WithdrawalClaims claims,
        bytes32 componentConfigHash,
        uint192 componentRelease
    ) LayerXComponent(Predeploys.CUSTODY_TOPOLOGY, componentConfigHash, componentRelease) {
        if (
            address(registry) == address(0) || address(bond) == address(0) || address(custodyVault) == address(0)
                || address(challenges) == address(0) || address(nullifiers) == address(0)
                || address(claims) == address(0) || address(challenges.registry()) != address(registry)
                || address(challenges.guarantorBond()) != address(bond)
                || address(claims.registry()) != address(registry)
                || address(claims.challengeManager()) != address(challenges)
                || address(claims.nullifierRegistry()) != address(nullifiers)
                || address(claims.vault()) != address(custodyVault)
        ) {
            revert InvalidCustodyTopology();
        }
        checkpointRegistry = registry;
        guarantorBond = bond;
        vault = custodyVault;
        challengeManager = challenges;
        withdrawalNullifiers = nullifiers;
        withdrawalClaims = claims;
        topologyHash = sha256(
            abi.encode(
                "LXP/Paxeer/custody-topology/v1",
                block.chainid,
                registry,
                bond,
                custodyVault,
                challenges,
                nullifiers,
                claims
            )
        );
    }
}
