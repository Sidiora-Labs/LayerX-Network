// SPDX-License-Identifier: LicenseRef-Centra-ai-Protocol
pragma solidity ^0.8.24;

interface IGuarantorEligibility {
    function protocolVersion() external view returns (uint16);

    function networkId() external view returns (uint32);

    function slashingAuthority() external view returns (address);

    function membershipVersion() external view returns (uint64);

    function bondedActive(bytes32 guarantorId, address signer, uint64 checkpointEpoch) external view returns (bool);
}
