// SPDX-License-Identifier: LicenseRef-Centra-ai-Protocol
pragma solidity ^0.8.24;

interface IGuarantorEligibility {
    function slashingAuthority() external view returns (address);

    function bondedActive(bytes32 guarantorId, address signer, uint64 checkpointEpoch) external view returns (bool);
}
