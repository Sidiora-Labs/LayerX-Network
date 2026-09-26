// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.30;

/// @dev Subset of the Foundry cheatcode interface used by these tests.
interface Vm {
    function addr(uint256 privateKey) external pure returns (address);
    function sign(uint256 privateKey, bytes32 digest) external pure returns (uint8 v, bytes32 r, bytes32 s);
    function prank(address sender) external;
    function deal(address account, uint256 balance) external;
    function chainId(uint256 newChainId) external;
    function expectRevert() external;
    function expectRevert(bytes calldata revertData) external;
    function expectRevert(bytes4 revertData) external;
    function expectEmit() external;
    function assume(bool condition) external pure;
}

abstract contract CheatTest {
    Vm internal constant vm = Vm(address(uint160(uint256(keccak256("hevm cheat code")))));

    function assertEq(uint256 a, uint256 b, string memory what) internal pure {
        require(a == b, what);
    }

    function assertEq(bytes32 a, bytes32 b, string memory what) internal pure {
        require(a == b, what);
    }

    function assertEq(address a, address b, string memory what) internal pure {
        require(a == b, what);
    }

    function assertTrue(bool a, string memory what) internal pure {
        require(a, what);
    }
}
