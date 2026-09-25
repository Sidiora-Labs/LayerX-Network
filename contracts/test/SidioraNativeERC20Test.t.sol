// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {Test} from "forge-std/Test.sol";
import {Vm} from "forge-std/Vm.sol";
import {ProxyAdmin} from "@openzeppelin/contracts/proxy/transparent/ProxyAdmin.sol";
import {
    TransparentUpgradeableProxy,
    ITransparentUpgradeableProxy
} from "@openzeppelin/contracts/proxy/transparent/TransparentUpgradeableProxy.sol";
import {IERC20Errors} from "@openzeppelin/contracts/interfaces/draft-IERC6093.sol";
import {SidioraNativeERC20} from "../src/SidioraNativeERC20.sol";
import {IBank, BANK_PRECOMPILE_ADDRESS} from "../src/precompiles/IBank.sol";

contract SidioraBankFixture {
    mapping(address => uint256) private balances;
    string private nativeDenom;
    uint256 private nativeSupply;
    uint8 private refusal;
    address private pointer;

    function configure(string memory denom_, address pointer_, address alice, address bob) external {
        nativeDenom = denom_;
        pointer = pointer_;
        balances[alice] = 2_000_000;
        balances[bob] = 1_000_000;
        nativeSupply = 3_000_000;
    }

    function setRefusal(uint8 refusal_) external {
        refusal = refusal_;
    }

    function balance(address account, string memory denom_) external view returns (uint256) {
        require(keccak256(bytes(denom_)) == keccak256(bytes(nativeDenom)), "unsupported denom");
        return balances[account];
    }

    function supply(string memory denom_) external view returns (uint256) {
        require(keccak256(bytes(denom_)) == keccak256(bytes(nativeDenom)), "unsupported denom");
        return nativeSupply;
    }

    function send(address from, address to, string memory denom_, uint256 amount) external returns (bool) {
        require(msg.sender == pointer, "unregistered pointer");
        require(keccak256(bytes(denom_)) == keccak256(bytes(nativeDenom)), "unsupported denom");
        if (refusal == 1) return false;
        require(refusal != 2, "bank refused");
        balances[from] -= amount;
        balances[to] += amount;
        return true;
    }
}

contract SidioraNativeERC20Test is Test {
    event Transfer(address indexed from, address indexed to, uint256 value);
    event Approval(address indexed owner, address indexed spender, uint256 value);

    address private constant SIDIORA = address(bytes20(hex"21f7b20a555199fa73a238b1a91fd0f549068fee"));
    bytes32 private constant IMPLEMENTATION_SLOT = bytes32(uint256(keccak256("eip1967.proxy.implementation")) - 1);
    bytes32 private constant ADMIN_SLOT = bytes32(uint256(keccak256("eip1967.proxy.admin")) - 1);
    bytes32 private constant BEACON_SLOT = bytes32(uint256(keccak256("eip1967.proxy.beacon")) - 1);

    SidioraNativeERC20 private token;
    SidioraNativeERC20 private implementation;
    SidioraBankFixture private bank;
    ProxyAdmin private admin;
    address private alice;
    address private bob;
    string private nativeDenom;

    function setUp() public {
        alice = makeAddr("alice");
        bob = makeAddr("bob");
        nativeDenom = string.concat("factory/", _moduleAccount(), "/usid");
        implementation = new SidioraNativeERC20();
        TransparentUpgradeableProxy proxy = new TransparentUpgradeableProxy(address(implementation), address(this), "");
        vm.etch(SIDIORA, address(proxy).code);
        vm.store(SIDIORA, IMPLEMENTATION_SLOT, vm.load(address(proxy), IMPLEMENTATION_SLOT));
        vm.store(SIDIORA, ADMIN_SLOT, vm.load(address(proxy), ADMIN_SLOT));
        admin = ProxyAdmin(address(uint160(uint256(vm.load(SIDIORA, ADMIN_SLOT)))));
        token = SidioraNativeERC20(SIDIORA);
        SidioraBankFixture fixture = new SidioraBankFixture();
        vm.etch(BANK_PRECOMPILE_ADDRESS, address(fixture).code);
        bank = SidioraBankFixture(BANK_PRECOMPILE_ADDRESS);
        bank.configure(nativeDenom, SIDIORA, alice, bob);
    }

    function _initialize() private {
        admin.upgradeAndCall(
            ITransparentUpgradeableProxy(SIDIORA), address(implementation), abi.encodeCall(token.initialize, ())
        );
    }

    function testMetadataAndDerivedDenom() public {
        _initialize();
        assertEq(address(token), SIDIORA);
        assertEq(token.name(), "Sidiora");
        assertEq(token.symbol(), "SID");
        assertEq(token.decimals(), 6);
        assertEq(token.denom(), nativeDenom);
    }

    function testBankBalanceAndSupply() public {
        _initialize();
        vm.expectCall(BANK_PRECOMPILE_ADDRESS, abi.encodeCall(IBank.balance, (alice, nativeDenom)));
        assertEq(token.balanceOf(alice), 2_000_000);
        vm.expectCall(BANK_PRECOMPILE_ADDRESS, abi.encodeCall(IBank.supply, (nativeDenom)));
        assertEq(token.totalSupply(), 3_000_000);
        assertEq(token.balanceOf(address(0)), 0);
    }

    function testTransferMovesBankBalanceAndEmits() public {
        _initialize();
        vm.expectCall(BANK_PRECOMPILE_ADDRESS, abi.encodeCall(IBank.send, (alice, bob, nativeDenom, 123_456)));
        vm.expectEmit(true, true, false, true, SIDIORA);
        emit Transfer(alice, bob, 123_456);
        vm.prank(alice);
        assertTrue(token.transfer(bob, 123_456));
        assertEq(token.balanceOf(alice), 1_876_544);
        assertEq(token.balanceOf(bob), 1_123_456);
        assertEq(token.totalSupply(), 3_000_000);
    }

    function testBankFalseRevertsWithoutEventOrBalanceChange() public {
        _initialize();
        bank.setRefusal(1);
        _assertRefusedTransfer();
    }

    function testBankRevertReturnsNamedErrorWithoutEventOrBalanceChange() public {
        _initialize();
        bank.setRefusal(2);
        _assertRefusedTransfer();
    }

    function _assertRefusedTransfer() private {
        vm.recordLogs();
        vm.prank(alice);
        vm.expectRevert(SidioraNativeERC20.BankTransferFailed.selector);
        token.transfer(bob, 123_456);
        Vm.Log[] memory entries = vm.getRecordedLogs();
        assertEq(entries.length, 0);
        assertEq(token.balanceOf(alice), 2_000_000);
        assertEq(token.balanceOf(bob), 1_000_000);
    }

    function testInsufficientBankBalanceReverts() public {
        _initialize();
        vm.prank(alice);
        vm.expectRevert(SidioraNativeERC20.BankTransferFailed.selector);
        token.transfer(bob, 2_000_001);
        assertEq(token.balanceOf(alice), 2_000_000);
        assertEq(token.balanceOf(bob), 1_000_000);
    }

    function testSecondInitializationReverts() public {
        _initialize();
        vm.expectRevert(SidioraNativeERC20.AlreadyInitialized.selector);
        token.initialize();
        assertEq(token.denom(), nativeDenom);
        assertEq(token.name(), "Sidiora");
    }

    function testImplementationInitializationIsDisabled() public {
        vm.expectRevert(SidioraNativeERC20.AlreadyInitialized.selector);
        implementation.initialize();
    }

    function testUninitializedProxyRefusesReadsAndWrites() public {
        vm.expectRevert(SidioraNativeERC20.NotInitialized.selector);
        token.balanceOf(alice);
        vm.expectRevert(SidioraNativeERC20.NotInitialized.selector);
        token.totalSupply();
        vm.prank(alice);
        vm.expectRevert(SidioraNativeERC20.NotInitialized.selector);
        token.transfer(bob, 1);
        vm.prank(alice);
        vm.expectRevert(SidioraNativeERC20.NotInitialized.selector);
        token.approve(bob, 1);
    }

    function testApproveAndTransferFrom() public {
        _initialize();
        vm.prank(bob);
        vm.expectRevert(abi.encodeWithSelector(IERC20Errors.ERC20InsufficientAllowance.selector, bob, 0, 150));
        token.transferFrom(alice, bob, 150);
        vm.expectEmit(true, true, false, true, SIDIORA);
        emit Approval(alice, bob, 200);
        vm.prank(alice);
        assertTrue(token.approve(bob, 200));
        vm.expectEmit(true, true, false, true, SIDIORA);
        emit Transfer(alice, bob, 150);
        vm.prank(bob);
        assertTrue(token.transferFrom(alice, bob, 150));
        assertEq(token.allowance(alice, bob), 50);
        assertEq(token.balanceOf(alice), 1_999_850);
        assertEq(token.balanceOf(bob), 1_000_150);
    }

    function testRefusedTransferFromPreservesAllowance() public {
        _initialize();
        vm.prank(alice);
        token.approve(bob, 200);
        bank.setRefusal(1);
        vm.prank(bob);
        vm.expectRevert(SidioraNativeERC20.BankTransferFailed.selector);
        token.transferFrom(alice, bob, 150);
        assertEq(token.allowance(alice, bob), 200);
        assertEq(token.balanceOf(alice), 2_000_000);
    }

    function testInfiniteAllowanceIsNotSpent() public {
        _initialize();
        vm.prank(alice);
        token.approve(bob, type(uint256).max);
        vm.prank(bob);
        token.transferFrom(alice, bob, 150);
        assertEq(token.allowance(alice, bob), type(uint256).max);
    }

    function testZeroAddressRefusals() public {
        _initialize();
        vm.prank(alice);
        vm.expectRevert(abi.encodeWithSelector(IERC20Errors.ERC20InvalidReceiver.selector, address(0)));
        token.transfer(address(0), 1);
        vm.prank(alice);
        vm.expectRevert(abi.encodeWithSelector(IERC20Errors.ERC20InvalidSpender.selector, address(0)));
        token.approve(address(0), 1);
    }

    function testZeroAndSelfTransfers() public {
        _initialize();
        vm.prank(alice);
        assertTrue(token.transfer(bob, 0));
        vm.prank(alice);
        assertTrue(token.transfer(alice, 100));
        assertEq(token.balanceOf(alice), 2_000_000);
        assertEq(token.balanceOf(bob), 1_000_000);
    }

    function testProxyStoragePreservedAndNamespaceLayout() public {
        for (uint256 i; i < 256; ++i) {
            vm.store(SIDIORA, bytes32(i), bytes32(i + 1000));
        }
        bytes32 legacyBalance = keccak256(abi.encode(alice, uint256(0)));
        bytes32 legacyAllowance = keccak256(abi.encode(bob, keccak256(abi.encode(alice, uint256(1)))));
        vm.store(SIDIORA, legacyBalance, bytes32(uint256(777)));
        vm.store(SIDIORA, legacyAllowance, bytes32(uint256(888)));
        vm.store(SIDIORA, BEACON_SLOT, bytes32(uint256(999)));
        bytes32 adminBefore = vm.load(SIDIORA, ADMIN_SLOT);
        _initialize();
        assertEq(token.allowance(alice, bob), 0);
        vm.prank(alice);
        token.approve(bob, 200);
        vm.prank(bob);
        token.transferFrom(alice, bob, 150);
        for (uint256 i; i < 256; ++i) {
            assertEq(vm.load(SIDIORA, bytes32(i)), bytes32(i + 1000));
        }
        assertEq(vm.load(SIDIORA, legacyBalance), bytes32(uint256(777)));
        assertEq(vm.load(SIDIORA, legacyAllowance), bytes32(uint256(888)));
        assertEq(vm.load(SIDIORA, IMPLEMENTATION_SLOT), bytes32(uint256(uint160(address(implementation)))));
        assertEq(vm.load(SIDIORA, ADMIN_SLOT), adminBefore);
        assertEq(vm.load(SIDIORA, BEACON_SLOT), bytes32(uint256(999)));
        uint256 base = uint256(
            keccak256(abi.encode(uint256(keccak256("paxeer.storage.SidioraNativeERC20")) - 1)) & ~bytes32(uint256(0xff))
        );
        assertEq(vm.load(SIDIORA, bytes32(base)), bytes32(uint256(1)));
        assertEq(vm.load(SIDIORA, bytes32(base + 1)), bytes32(bytes(nativeDenom).length * 2 + 1));
        assertEq(vm.load(SIDIORA, bytes32(base + 2)), bytes32("Sidiora") | bytes32(uint256(14)));
        assertEq(vm.load(SIDIORA, bytes32(base + 3)), bytes32("SID") | bytes32(uint256(6)));
        assertEq(vm.load(SIDIORA, bytes32(base + 4)), bytes32(uint256(6)));
        bytes32 allowanceSlot = keccak256(abi.encode(bob, keccak256(abi.encode(alice, base + 5))));
        assertEq(vm.load(SIDIORA, allowanceSlot), bytes32(uint256(50)));
        assertEq(token.denom(), nativeDenom);
    }

    function testViewsWriteNothing() public {
        _initialize();
        vm.record();
        token.name();
        token.symbol();
        token.decimals();
        token.denom();
        token.balanceOf(alice);
        token.totalSupply();
        token.allowance(alice, bob);
        (, bytes32[] memory tokenWrites) = vm.accesses(SIDIORA);
        (, bytes32[] memory bankWrites) = vm.accesses(BANK_PRECOMPILE_ADDRESS);
        assertEq(tokenWrites.length, 0);
        assertEq(bankWrites.length, 0);
    }

    function _moduleAccount() private pure returns (string memory) {
        bytes memory prefix = "pax";
        bytes memory values = new bytes(45);
        for (uint256 i; i < prefix.length; ++i) {
            values[i] = bytes1(uint8(prefix[i]) >> 5);
            values[i + 4] = bytes1(uint8(prefix[i]) & 31);
        }
        bytes20 hash = bytes20(sha256("layerxbridge"));
        uint256 accumulator;
        uint256 bits;
        uint256 offset = 7;
        for (uint256 i; i < hash.length; ++i) {
            accumulator = (accumulator << 8) | uint8(hash[i]);
            bits += 8;
            while (bits >= 5) {
                bits -= 5;
                values[offset++] = bytes1(uint8((accumulator >> bits) & 31));
            }
        }
        uint256 checksum = 1;
        uint256[5] memory generators = [uint256(0x3b6a57b2), 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3];
        for (uint256 i; i < values.length; ++i) {
            uint256 top = checksum >> 25;
            checksum = ((checksum & 0x1ffffff) << 5) ^ uint8(values[i]);
            for (uint256 j; j < 5; ++j) {
                if ((top & (1 << j)) != 0) checksum ^= generators[j];
            }
        }
        checksum ^= 1;
        bytes memory alphabet = "qpzry9x8gf2tvdw0s3jn54khce6mua7l";
        bytes memory encoded = new bytes(38);
        for (uint256 i; i < 32; ++i) {
            encoded[i] = alphabet[uint8(values[7 + i])];
        }
        for (uint256 i; i < 6; ++i) {
            encoded[32 + i] = alphabet[(checksum >> (25 - 5 * i)) & 31];
        }
        return string.concat(string(prefix), "1", string(encoded));
    }
}
