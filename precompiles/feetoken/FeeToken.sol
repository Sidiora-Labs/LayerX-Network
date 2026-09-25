// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

address constant FEE_TOKEN_PRECOMPILE_ADDRESS = 0x0000000000000000000000000000000000001018;
IFeeToken constant FEE_TOKEN_CONTRACT = IFeeToken(FEE_TOKEN_PRECOMPILE_ADDRESS);

interface IFeeToken {
    function setFeeDenom(string calldata denom) external;
    function getFeeDenom(address account) external view returns (string memory denom);
    function clearFeeDenom() external;
}
