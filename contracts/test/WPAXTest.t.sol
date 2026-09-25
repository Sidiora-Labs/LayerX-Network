// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.13;

import {Test} from "forge-std/Test.sol";
import {WPAX} from "../src/WPAX.sol";

contract WPAXTest is Test {
    event Approval(address indexed src, address indexed guy, uint256 wad);
    event Transfer(address indexed src, address indexed dst, uint256 wad);
    event Deposit(address indexed dst, uint256 wad);
    event Withdrawal(address indexed src, uint256 wad);

    uint256 internal constant NAME_SLOT = 0;
    uint256 internal constant SYMBOL_SLOT = 1;
    uint256 internal constant DECIMALS_SLOT = 2;
    uint256 internal constant BALANCE_OF_SLOT = 3;
    uint256 internal constant ALLOWANCE_SLOT = 4;

    WPAX internal wpax;
    address internal alice;
    address internal bob;
    address internal carol;

    function setUp() public {
        wpax = new WPAX();
        alice = makeAddr("alice");
        bob = makeAddr("bob");
        carol = makeAddr("carol");
        vm.deal(alice, 100 ether);
        vm.deal(bob, 100 ether);
    }

    function testMetadataNamesTheCoin() public view {
        assertEq(wpax.name(), "Wrapped Paxeer");
        assertEq(wpax.symbol(), "WPAX");
        assertEq(wpax.decimals(), 18);
    }

    function testMetadataKeepsItsStorageSlots() public view {
        assertEq(vm.load(address(wpax), bytes32(NAME_SLOT)), _shortStringSlot("Wrapped Paxeer"));
        assertEq(vm.load(address(wpax), bytes32(SYMBOL_SLOT)), _shortStringSlot("WPAX"));
        assertEq(uint256(vm.load(address(wpax), bytes32(DECIMALS_SLOT))), 18);
    }

    function testFunctionSelectorsAreUnchanged() public view {
        assertEq(wpax.name.selector, bytes4(0x06fdde03));
        assertEq(wpax.symbol.selector, bytes4(0x95d89b41));
        assertEq(wpax.decimals.selector, bytes4(0x313ce567));
        assertEq(wpax.balanceOf.selector, bytes4(0x70a08231));
        assertEq(wpax.allowance.selector, bytes4(0xdd62ed3e));
        assertEq(wpax.totalSupply.selector, bytes4(0x18160ddd));
        assertEq(wpax.deposit.selector, bytes4(0xd0e30db0));
        assertEq(wpax.withdraw.selector, bytes4(0x2e1a7d4d));
        assertEq(wpax.approve.selector, bytes4(0x095ea7b3));
        assertEq(wpax.transfer.selector, bytes4(0xa9059cbb));
        assertEq(wpax.transferFrom.selector, bytes4(0x23b872dd));
    }

    function testEventSignaturesAreUnchanged() public pure {
        assertEq(Approval.selector, keccak256("Approval(address,address,uint256)"));
        assertEq(Transfer.selector, keccak256("Transfer(address,address,uint256)"));
        assertEq(Deposit.selector, keccak256("Deposit(address,uint256)"));
        assertEq(Withdrawal.selector, keccak256("Withdrawal(address,uint256)"));
    }

    function testDepositCreditsTheSenderAndEmits() public {
        vm.expectEmit(true, false, false, true, address(wpax));
        emit Deposit(alice, 3 ether);

        vm.prank(alice);
        wpax.deposit{value: 3 ether}();

        assertEq(wpax.balanceOf(alice), 3 ether);
        assertEq(wpax.totalSupply(), 3 ether);
        assertEq(address(wpax).balance, 3 ether);
        assertEq(alice.balance, 97 ether);
        assertEq(uint256(vm.load(address(wpax), _mappingSlot(alice, BALANCE_OF_SLOT))), 3 ether);
    }

    function testReceiveAndFallbackDeposit() public {
        vm.prank(alice);
        (bool received,) = address(wpax).call{value: 1 ether}("");
        assertTrue(received);
        assertEq(wpax.balanceOf(alice), 1 ether);

        vm.prank(alice);
        (bool fellBack,) = address(wpax).call{value: 2 ether}(abi.encodeWithSignature("notAFunction()"));
        assertTrue(fellBack);
        assertEq(wpax.balanceOf(alice), 3 ether);
        assertEq(wpax.totalSupply(), 3 ether);
    }

    function testWithdrawDebitsTheSenderAndPaysOut() public {
        vm.prank(alice);
        wpax.deposit{value: 5 ether}();

        vm.expectEmit(true, false, false, true, address(wpax));
        emit Withdrawal(alice, 2 ether);

        vm.prank(alice);
        wpax.withdraw(2 ether);

        assertEq(wpax.balanceOf(alice), 3 ether);
        assertEq(wpax.totalSupply(), 3 ether);
        assertEq(address(wpax).balance, 3 ether);
        assertEq(alice.balance, 97 ether);
    }

    function testWithdrawRevertsAboveBalance() public {
        vm.prank(alice);
        wpax.deposit{value: 1 ether}();

        vm.prank(alice);
        vm.expectRevert();
        wpax.withdraw(1 ether + 1);
    }

    function testTransferMovesBalanceAndEmits() public {
        vm.prank(alice);
        wpax.deposit{value: 4 ether}();

        vm.expectEmit(true, true, false, true, address(wpax));
        emit Transfer(alice, bob, 1.5 ether);

        vm.prank(alice);
        assertTrue(wpax.transfer(bob, 1.5 ether));

        assertEq(wpax.balanceOf(alice), 2.5 ether);
        assertEq(wpax.balanceOf(bob), 1.5 ether);
        assertEq(wpax.totalSupply(), 4 ether);
    }

    function testApprovedTransferFromSpendsTheAllowance() public {
        vm.prank(alice);
        wpax.deposit{value: 4 ether}();

        vm.expectEmit(true, true, false, true, address(wpax));
        emit Approval(alice, bob, 3 ether);

        vm.prank(alice);
        assertTrue(wpax.approve(bob, 3 ether));
        assertEq(wpax.allowance(alice, bob), 3 ether);
        assertEq(
            uint256(vm.load(address(wpax), _mappingSlot(bob, uint256(_mappingSlot(alice, ALLOWANCE_SLOT))))), 3 ether
        );

        vm.expectEmit(true, true, false, true, address(wpax));
        emit Transfer(alice, carol, 2 ether);

        vm.prank(bob);
        assertTrue(wpax.transferFrom(alice, carol, 2 ether));

        assertEq(wpax.allowance(alice, bob), 1 ether);
        assertEq(wpax.balanceOf(alice), 2 ether);
        assertEq(wpax.balanceOf(carol), 2 ether);
        assertEq(wpax.balanceOf(bob), 0);
    }

    function testInfiniteAllowanceIsNotSpent() public {
        vm.prank(alice);
        wpax.deposit{value: 4 ether}();

        vm.prank(alice);
        wpax.approve(bob, type(uint256).max);

        vm.prank(bob);
        assertTrue(wpax.transferFrom(alice, carol, 4 ether));

        assertEq(wpax.allowance(alice, bob), type(uint256).max);
        assertEq(wpax.balanceOf(alice), 0);
        assertEq(wpax.balanceOf(carol), 4 ether);
    }

    function testTransferFromRevertsAboveAllowance() public {
        vm.prank(alice);
        wpax.deposit{value: 4 ether}();

        vm.prank(alice);
        wpax.approve(bob, 1 ether);

        vm.prank(bob);
        vm.expectRevert();
        wpax.transferFrom(alice, carol, 2 ether);
    }

    function _mappingSlot(address key, uint256 slot) internal pure returns (bytes32) {
        return keccak256(abi.encode(key, slot));
    }

    function _shortStringSlot(string memory value) internal pure returns (bytes32 packed) {
        bytes memory raw = bytes(value);
        require(raw.length < 32, "WPAXTest: long string");
        assembly {
            packed := or(mload(add(raw, 32)), mul(2, mload(raw)))
        }
    }
}
