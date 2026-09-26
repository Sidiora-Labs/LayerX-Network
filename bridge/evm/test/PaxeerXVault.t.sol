// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.30;

import {Ownable} from "@openzeppelin/contracts/access/Ownable.sol";
import {Ownable2Step} from "@openzeppelin/contracts/access/Ownable2Step.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";

import {PaxeerXVault} from "../src/PaxeerXVault.sol";
import {BridgeAttestation} from "../src/BridgeAttestation.sol";
import {CheatTest} from "./Vm.sol";
import {TestToken} from "./TestToken.sol";

contract PaxeerXVaultTest is CheatTest {
    address internal constant OWNER = address(0xB0B);
    address internal constant USER = address(0xA11CE);
    address internal constant RECIPIENT = address(0xCAFE);
    address internal constant NATIVE = address(0);
    bytes32 internal constant PAXEER_RECIPIENT = bytes32(uint256(0xFEED));
    uint256 internal constant PER_TX = 100 ether;
    uint256 internal constant TOTAL = 250 ether;

    PaxeerXVault internal vault;
    TestToken internal token;
    uint256[3] internal keys = [uint256(0xA1), uint256(0xA2), uint256(0xA3)];
    uint256 internal constant OUTSIDER_KEY = 0xBAD;

    function setUp() public {
        address[] memory attestors = new address[](3);
        for (uint256 i = 0; i < 3; ++i) {
            attestors[i] = vm.addr(keys[i]);
        }
        vault = new PaxeerXVault(OWNER, attestors, 2);
        token = new TestToken();
        vm.prank(OWNER);
        vault.setCap(address(token), PER_TX, TOTAL);
        vm.prank(OWNER);
        vault.setCap(NATIVE, PER_TX, TOTAL);
    }

    function signature(uint256 key, bytes32 digest) internal pure returns (bytes memory) {
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(key, digest);
        return abi.encodePacked(r, s, v);
    }

    /// @dev Signs with the given keys and orders signatures by ascending signer.
    function signSorted(uint256[] memory signers, bytes32 digest) internal pure returns (bytes[] memory sigs) {
        uint256 n = signers.length;
        for (uint256 i = 0; i < n; ++i) {
            for (uint256 j = i + 1; j < n; ++j) {
                if (vm.addr(signers[j]) < vm.addr(signers[i])) {
                    (signers[i], signers[j]) = (signers[j], signers[i]);
                }
            }
        }
        sigs = new bytes[](n);
        for (uint256 i = 0; i < n; ++i) {
            sigs[i] = signature(signers[i], digest);
        }
    }

    function twoKeys() internal view returns (uint256[] memory k) {
        k = new uint256[](2);
        k[0] = keys[0];
        k[1] = keys[2];
    }

    function depositToken(uint256 amount) internal {
        token.mint(USER, amount);
        vm.prank(USER);
        token.approve(address(vault), amount);
        vm.prank(USER);
        vault.deposit(address(token), amount, PAXEER_RECIPIENT);
    }

    function releaseSigs(bytes32 txHash, uint64 nonce, address asset, uint256 amount)
        internal
        view
        returns (bytes[] memory)
    {
        return signSorted(twoKeys(), vault.releaseDigest(txHash, nonce, RECIPIENT, asset, amount));
    }

    // ---- deposits ----

    function test_DepositERC20() public {
        token.mint(USER, 40 ether);
        vm.prank(USER);
        token.approve(address(vault), 40 ether);
        vm.expectEmit();
        emit PaxeerXVault.BridgeDeposit(address(token), 40 ether, USER, PAXEER_RECIPIENT, 0);
        vm.prank(USER);
        vault.deposit(address(token), 40 ether, PAXEER_RECIPIENT);
        assertEq(token.balanceOf(address(vault)), 40 ether, "vault balance");
        assertEq(token.balanceOf(USER), 0, "user balance");
        assertEq(vault.outstanding(address(token)), 40 ether, "outstanding");
        assertEq(vault.depositNonce(), 1, "nonce");
    }

    function test_DepositNative() public {
        vm.deal(USER, 5 ether);
        depositToken(1 ether);
        vm.expectEmit();
        emit PaxeerXVault.BridgeDeposit(NATIVE, 5 ether, USER, PAXEER_RECIPIENT, 1);
        vm.prank(USER);
        vault.depositNative{value: 5 ether}(PAXEER_RECIPIENT);
        assertEq(address(vault).balance, 5 ether, "vault eth");
        assertEq(vault.outstanding(NATIVE), 5 ether, "outstanding");
        assertEq(vault.depositNonce(), 2, "nonce");
    }

    function test_DepositRejectsNativeAssetThroughErc20Path() public {
        vm.expectRevert(PaxeerXVault.UseDepositNative.selector);
        vm.prank(USER);
        vault.deposit(NATIVE, 1 ether, PAXEER_RECIPIENT);
    }

    function test_DepositRefusesUnconfiguredAsset() public {
        TestToken other = new TestToken();
        other.mint(USER, 1 ether);
        vm.prank(USER);
        other.approve(address(vault), 1 ether);
        vm.expectRevert(abi.encodeWithSelector(PaxeerXVault.AssetNotEnabled.selector, address(other)));
        vm.prank(USER);
        vault.deposit(address(other), 1 ether, PAXEER_RECIPIENT);
    }

    function test_DepositRefusesOverPerTxCap() public {
        token.mint(USER, PER_TX + 1);
        vm.prank(USER);
        token.approve(address(vault), PER_TX + 1);
        vm.expectRevert(abi.encodeWithSelector(PaxeerXVault.PerTxCapExceeded.selector, PER_TX + 1, PER_TX));
        vm.prank(USER);
        vault.deposit(address(token), PER_TX + 1, PAXEER_RECIPIENT);
    }

    function test_DepositRefusesOverOutstandingCap() public {
        depositToken(PER_TX);
        depositToken(PER_TX);
        token.mint(USER, 51 ether);
        vm.prank(USER);
        token.approve(address(vault), 51 ether);
        vm.expectRevert(abi.encodeWithSelector(PaxeerXVault.TotalCapExceeded.selector, TOTAL + 1 ether, TOTAL));
        vm.prank(USER);
        vault.deposit(address(token), 51 ether, PAXEER_RECIPIENT);
    }

    function test_DepositRefusesWhenPaused() public {
        vm.prank(OWNER);
        vault.pause();
        vm.deal(USER, 1 ether);
        vm.expectRevert(PaxeerXVault.WhenPaused.selector);
        vm.prank(USER);
        vault.depositNative{value: 1 ether}(PAXEER_RECIPIENT);
    }

    // ---- releases ----

    function test_ReleaseAtThresholdMarksNullifier() public {
        depositToken(60 ether);
        bytes32 txHash = keccak256("paxeer-burn-1");
        bytes[] memory sigs = releaseSigs(txHash, 9, address(token), 25 ether);
        vm.expectEmit();
        emit PaxeerXVault.BridgeRelease(address(token), 25 ether, RECIPIENT, txHash, 9);
        vault.release(address(token), 25 ether, RECIPIENT, txHash, 9, sigs);
        assertEq(token.balanceOf(RECIPIENT), 25 ether, "recipient");
        assertEq(vault.outstanding(address(token)), 35 ether, "outstanding");
        assertTrue(vault.nullified(vault.nullifierOf(txHash, 9)), "nullifier");
        assertTrue(!vault.nullified(vault.nullifierOf(txHash, 10)), "other nonce untouched");
    }

    function test_ReleaseNativeWithAllAttestors() public {
        vm.deal(USER, 10 ether);
        vm.prank(USER);
        vault.depositNative{value: 10 ether}(PAXEER_RECIPIENT);
        bytes32 txHash = keccak256("paxeer-burn-native");
        uint256[] memory all = new uint256[](3);
        for (uint256 i = 0; i < 3; ++i) {
            all[i] = keys[i];
        }
        bytes[] memory sigs = signSorted(all, vault.releaseDigest(txHash, 1, RECIPIENT, NATIVE, 4 ether));
        vault.release(NATIVE, 4 ether, RECIPIENT, txHash, 1, sigs);
        assertEq(RECIPIENT.balance, 4 ether, "recipient eth");
        assertEq(vault.outstanding(NATIVE), 6 ether, "outstanding");
    }

    function test_ReleaseRefusesBelowThreshold() public {
        depositToken(10 ether);
        bytes32 txHash = keccak256("burn");
        uint256[] memory one = new uint256[](1);
        one[0] = keys[1];
        bytes[] memory sigs = signSorted(one, vault.releaseDigest(txHash, 1, RECIPIENT, address(token), 1 ether));
        vm.expectRevert(abi.encodeWithSelector(PaxeerXVault.BelowThreshold.selector, 1, 2));
        vault.release(address(token), 1 ether, RECIPIENT, txHash, 1, sigs);
    }

    function test_ReleaseRefusesDuplicateSigner() public {
        depositToken(10 ether);
        bytes32 txHash = keccak256("burn");
        bytes32 digest = vault.releaseDigest(txHash, 1, RECIPIENT, address(token), 1 ether);
        bytes[] memory sigs = new bytes[](2);
        sigs[0] = signature(keys[0], digest);
        sigs[1] = signature(keys[0], digest);
        vm.expectRevert(abi.encodeWithSelector(PaxeerXVault.SignersNotAscending.selector, vm.addr(keys[0])));
        vault.release(address(token), 1 ether, RECIPIENT, txHash, 1, sigs);
    }

    function test_ReleaseRefusesUnknownSigner() public {
        depositToken(10 ether);
        bytes32 txHash = keccak256("burn");
        uint256[] memory k = new uint256[](2);
        k[0] = keys[0];
        k[1] = OUTSIDER_KEY;
        bytes[] memory sigs = signSorted(k, vault.releaseDigest(txHash, 1, RECIPIENT, address(token), 1 ether));
        vm.expectRevert(abi.encodeWithSelector(PaxeerXVault.UnknownSigner.selector, vm.addr(OUTSIDER_KEY)));
        vault.release(address(token), 1 ether, RECIPIENT, txHash, 1, sigs);
    }

    function test_ReleaseRefusesSignatureForOtherAmount() public {
        depositToken(10 ether);
        bytes32 txHash = keccak256("burn");
        bytes[] memory sigs = releaseSigs(txHash, 1, address(token), 1 ether);
        vm.expectRevert();
        vault.release(address(token), 2 ether, RECIPIENT, txHash, 1, sigs);
        assertTrue(!vault.nullified(vault.nullifierOf(txHash, 1)), "nullifier untouched");
    }

    function test_ReleaseRefusesSignatureForOtherChain() public {
        depositToken(10 ether);
        bytes32 txHash = keccak256("burn");
        bytes[] memory sigs = releaseSigs(txHash, 1, address(token), 1 ether);
        vm.chainId(block.chainid + 1);
        vm.expectRevert();
        vault.release(address(token), 1 ether, RECIPIENT, txHash, 1, sigs);
    }

    function test_ReleaseRefusesOverPerTxCap() public {
        depositToken(PER_TX);
        vm.prank(OWNER);
        vault.setCap(address(token), 10 ether, TOTAL);
        bytes32 txHash = keccak256("burn");
        bytes[] memory sigs = releaseSigs(txHash, 1, address(token), 11 ether);
        vm.expectRevert(abi.encodeWithSelector(PaxeerXVault.PerTxCapExceeded.selector, 11 ether, 10 ether));
        vault.release(address(token), 11 ether, RECIPIENT, txHash, 1, sigs);
    }

    function test_ReleaseRefusesOverOutstanding() public {
        depositToken(5 ether);
        bytes32 txHash = keccak256("burn");
        bytes[] memory sigs = releaseSigs(txHash, 1, address(token), 6 ether);
        vm.expectRevert(abi.encodeWithSelector(PaxeerXVault.InsufficientOutstanding.selector, 6 ether, 5 ether));
        vault.release(address(token), 6 ether, RECIPIENT, txHash, 1, sigs);
    }

    function test_ReleaseRefusesWhenPaused() public {
        depositToken(10 ether);
        bytes32 txHash = keccak256("burn");
        bytes[] memory sigs = releaseSigs(txHash, 1, address(token), 1 ether);
        vm.prank(OWNER);
        vault.pause();
        vm.expectRevert(PaxeerXVault.WhenPaused.selector);
        vault.release(address(token), 1 ether, RECIPIENT, txHash, 1, sigs);
        vm.prank(OWNER);
        vault.unpause();
        vault.release(address(token), 1 ether, RECIPIENT, txHash, 1, sigs);
        assertEq(token.balanceOf(RECIPIENT), 1 ether, "released after unpause");
    }

    function test_ReleaseRefusesReplayedNullifier() public {
        depositToken(10 ether);
        bytes32 txHash = keccak256("burn");
        bytes[] memory sigs = releaseSigs(txHash, 3, address(token), 1 ether);
        vault.release(address(token), 1 ether, RECIPIENT, txHash, 3, sigs);
        vm.expectRevert(abi.encodeWithSelector(PaxeerXVault.NullifierUsed.selector, vault.nullifierOf(txHash, 3)));
        vault.release(address(token), 1 ether, RECIPIENT, txHash, 3, sigs);
        assertEq(token.balanceOf(RECIPIENT), 1 ether, "released once");
    }

    function test_ReleaseRefusesHighSSignature() public {
        depositToken(10 ether);
        bytes32 txHash = keccak256("burn");
        bytes32 digest = vault.releaseDigest(txHash, 1, RECIPIENT, address(token), 1 ether);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(keys[0], digest);
        uint256 n = 0xfffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141;
        bytes[] memory sigs = new bytes[](2);
        sigs[0] = abi.encodePacked(r, bytes32(n - uint256(s)), v == 27 ? uint8(28) : uint8(27));
        sigs[1] = signature(keys[1], digest);
        vm.expectRevert(PaxeerXVault.InvalidSignature.selector);
        vault.release(address(token), 1 ether, RECIPIENT, txHash, 1, sigs);
    }

    // ---- owner-only configuration ----

    function notOwner(address account) internal pure returns (bytes memory) {
        return abi.encodeWithSelector(Ownable.OwnableUnauthorizedAccount.selector, account);
    }

    function test_OwnerOnlySetters() public {
        address[] memory attestors = new address[](1);
        attestors[0] = vm.addr(OUTSIDER_KEY);

        vm.expectRevert(notOwner(USER));
        vm.prank(USER);
        vault.setAttestors(attestors, 1);

        vm.expectRevert(notOwner(USER));
        vm.prank(USER);
        vault.setCap(address(token), 1, 1);

        vm.expectRevert(notOwner(USER));
        vm.prank(USER);
        vault.pause();

        vm.prank(OWNER);
        vault.pause();
        vm.expectRevert(notOwner(USER));
        vm.prank(USER);
        vault.unpause();

        vm.expectRevert(notOwner(USER));
        vm.prank(USER);
        vault.transferOwnership(USER);

        vm.expectRevert(notOwner(USER));
        vm.prank(USER);
        vault.rescue(address(token), USER);
    }

    function test_SetAttestorsReplacesSetAndEmits() public {
        depositToken(10 ether);
        address[] memory attestors = new address[](1);
        attestors[0] = vm.addr(OUTSIDER_KEY);
        vm.expectEmit();
        emit PaxeerXVault.AttestorsSet(attestors, 1);
        vm.prank(OWNER);
        vault.setAttestors(attestors, 1);
        assertEq(vault.threshold(), 1, "threshold");
        assertTrue(!vault.isAttestor(vm.addr(keys[0])), "old attestor removed");
        assertTrue(vault.isAttestor(vm.addr(OUTSIDER_KEY)), "new attestor");

        bytes32 txHash = keccak256("burn");
        bytes[] memory oldSigs = releaseSigs(txHash, 1, address(token), 1 ether);
        vm.expectRevert(abi.encodeWithSelector(PaxeerXVault.UnknownSigner.selector, lowestOf(twoKeys())));
        vault.release(address(token), 1 ether, RECIPIENT, txHash, 1, oldSigs);

        bytes[] memory sigs = new bytes[](1);
        sigs[0] = signature(OUTSIDER_KEY, vault.releaseDigest(txHash, 1, RECIPIENT, address(token), 1 ether));
        vault.release(address(token), 1 ether, RECIPIENT, txHash, 1, sigs);
        assertEq(token.balanceOf(RECIPIENT), 1 ether, "released by new set");
    }

    function lowestOf(uint256[] memory k) internal pure returns (address lowest) {
        lowest = vm.addr(k[0]);
        for (uint256 i = 1; i < k.length; ++i) {
            if (vm.addr(k[i]) < lowest) lowest = vm.addr(k[i]);
        }
    }

    function test_SetAttestorsRejectsBadInput() public {
        address[] memory attestors = new address[](2);
        attestors[0] = vm.addr(keys[0]);
        attestors[1] = vm.addr(keys[0]);
        vm.expectRevert(abi.encodeWithSelector(PaxeerXVault.InvalidAttestor.selector, vm.addr(keys[0])));
        vm.prank(OWNER);
        vault.setAttestors(attestors, 1);

        attestors[1] = address(0);
        vm.expectRevert(abi.encodeWithSelector(PaxeerXVault.InvalidAttestor.selector, address(0)));
        vm.prank(OWNER);
        vault.setAttestors(attestors, 1);

        attestors[1] = vm.addr(keys[1]);
        vm.expectRevert(abi.encodeWithSelector(PaxeerXVault.InvalidThreshold.selector, 3, 2));
        vm.prank(OWNER);
        vault.setAttestors(attestors, 3);

        vm.expectRevert(abi.encodeWithSelector(PaxeerXVault.InvalidThreshold.selector, 0, 2));
        vm.prank(OWNER);
        vault.setAttestors(attestors, 0);
    }

    function test_SetCapEmitsAndApplies() public {
        vm.expectEmit();
        emit PaxeerXVault.CapSet(address(token), 1 ether, 2 ether);
        vm.prank(OWNER);
        vault.setCap(address(token), 1 ether, 2 ether);
        (uint256 perTx, uint256 total) = vault.caps(address(token));
        assertEq(perTx, 1 ether, "perTx");
        assertEq(total, 2 ether, "total");
    }

    function test_PauseUnpauseEmit() public {
        vm.expectEmit();
        emit PaxeerXVault.Paused(OWNER);
        vm.prank(OWNER);
        vault.pause();
        vm.expectEmit();
        emit PaxeerXVault.Unpaused(OWNER);
        vm.prank(OWNER);
        vault.unpause();
    }

    // ---- digest layout ----

    function appendBE(bytes memory buf, uint256 at, uint256 value, uint256 width) internal pure returns (uint256) {
        for (uint256 i = 0; i < width; ++i) {
            buf[at + i] = bytes1(uint8(value >> (8 * (width - 1 - i))));
        }
        return at + width;
    }

    function appendAscii(bytes memory buf, uint256 at, string memory s) internal pure returns (uint256) {
        bytes memory b = bytes(s);
        for (uint256 i = 0; i < b.length; ++i) {
            buf[at + i] = b[i];
        }
        return at + b.length;
    }

    function referenceOutbound(
        uint256 chainId,
        address vault_,
        bytes32 txHash,
        uint64 nonce,
        address recipient,
        address asset,
        uint256 amount
    ) internal pure returns (bytes memory buf) {
        buf = new bytes(185);
        uint256 at = appendAscii(buf, 0, "PAXEERX_BRIDGE_OUT_V1");
        at = appendBE(buf, at, chainId, 32);
        at = appendBE(buf, at, uint160(vault_), 20);
        at = appendBE(buf, at, uint256(txHash), 32);
        at = appendBE(buf, at, nonce, 8);
        at = appendBE(buf, at, uint160(recipient), 20);
        at = appendBE(buf, at, uint160(asset), 20);
        at = appendBE(buf, at, amount, 32);
        require(at == 185, "reference length");
    }

    function referenceInbound(
        uint256 chainId,
        address vault_,
        bytes32 txHash,
        uint64 logIndex,
        bytes32 paxeerRecipient,
        address asset,
        uint256 amount
    ) internal pure returns (bytes memory buf) {
        buf = new bytes(196);
        uint256 at = appendAscii(buf, 0, "PAXEERX_BRIDGE_IN_V1");
        at = appendBE(buf, at, chainId, 32);
        at = appendBE(buf, at, uint160(vault_), 20);
        at = appendBE(buf, at, uint256(txHash), 32);
        at = appendBE(buf, at, logIndex, 8);
        at = appendBE(buf, at, uint256(paxeerRecipient), 32);
        at = appendBE(buf, at, uint160(asset), 20);
        at = appendBE(buf, at, amount, 32);
        require(at == 196, "reference length");
    }

    function testFuzz_OutboundDigestLayout(
        uint256 chainId,
        address vault_,
        bytes32 txHash,
        uint64 nonce,
        address recipient,
        address asset,
        uint256 amount
    ) public pure {
        bytes memory expected = referenceOutbound(chainId, vault_, txHash, nonce, recipient, asset, amount);
        bytes memory actual =
            BridgeAttestation.outboundPreimage(chainId, vault_, txHash, nonce, recipient, asset, amount);
        assertEq(actual.length, BridgeAttestation.OUT_PREIMAGE_LENGTH, "out length");
        assertEq(keccak256(actual), keccak256(expected), "out preimage");
        assertEq(
            BridgeAttestation.outboundDigest(chainId, vault_, txHash, nonce, recipient, asset, amount),
            keccak256(expected),
            "out digest"
        );
    }

    function testFuzz_InboundDigestLayout(
        uint256 chainId,
        address vault_,
        bytes32 txHash,
        uint64 logIndex,
        bytes32 paxeerRecipient,
        address asset,
        uint256 amount
    ) public pure {
        bytes memory expected = referenceInbound(chainId, vault_, txHash, logIndex, paxeerRecipient, asset, amount);
        bytes memory actual =
            BridgeAttestation.inboundPreimage(chainId, vault_, txHash, logIndex, paxeerRecipient, asset, amount);
        assertEq(actual.length, BridgeAttestation.IN_PREIMAGE_LENGTH, "in length");
        assertEq(keccak256(actual), keccak256(expected), "in preimage");
        assertEq(
            BridgeAttestation.inboundDigest(chainId, vault_, txHash, logIndex, paxeerRecipient, asset, amount),
            keccak256(expected),
            "in digest"
        );
    }

    function testFuzz_VaultDigestsBindChainAndAddress(
        bytes32 txHash,
        uint64 nonce,
        address recipient,
        address asset,
        uint256 amount
    ) public view {
        assertEq(
            vault.releaseDigest(txHash, nonce, recipient, asset, amount),
            keccak256(referenceOutbound(block.chainid, address(vault), txHash, nonce, recipient, asset, amount)),
            "vault release digest"
        );
        assertEq(
            vault.depositDigest(txHash, nonce, bytes32(uint256(uint160(recipient))), asset, amount),
            keccak256(
                referenceInbound(
                    block.chainid, address(vault), txHash, nonce, bytes32(uint256(uint160(recipient))), asset, amount
                )
            ),
            "vault deposit digest"
        );
    }

    function test_DigestVectorsFromAttestationDoc() public pure {
        address v = 0x1111111111111111111111111111111111111111;
        bytes32 txHash = 0x2222222222222222222222222222222222222222222222222222222222222222;
        address recipient = 0x3333333333333333333333333333333333333333;
        address asset = 0x4444444444444444444444444444444444444444;
        bytes32 paxeerRecipient = 0x5555555555555555555555555555555555555555555555555555555555555555;
        assertEq(
            BridgeAttestation.outboundDigest(1, v, txHash, 7, recipient, asset, 1 ether),
            0xbd35888e4b158986238ce7abe73957702e2f6e78fe6157197878ebd13edf5b37,
            "out vector"
        );
        assertEq(
            BridgeAttestation.inboundDigest(1, v, txHash, 7, paxeerRecipient, asset, 1 ether),
            0x511964ae9566f9536604258667400b0d76335e2a6e60ab0f700bf2433bc97918,
            "in vector"
        );
    }

    // ---- two-step ownership ----

    function test_ConstructorRefusesZeroOwner() public {
        address[] memory attestors = new address[](1);
        attestors[0] = vm.addr(keys[0]);
        vm.expectRevert(abi.encodeWithSelector(Ownable.OwnableInvalidOwner.selector, address(0)));
        new PaxeerXVault(address(0), attestors, 1);
    }

    function test_OwnershipTransferIsTwoStep() public {
        vm.expectEmit();
        emit Ownable2Step.OwnershipTransferStarted(OWNER, USER);
        vm.prank(OWNER);
        vault.transferOwnership(USER);
        assertEq(vault.owner(), OWNER, "owner unchanged by the proposal");
        assertEq(vault.pendingOwner(), USER, "pending owner");

        // The sitting owner keeps every power until the transfer completes.
        vm.prank(OWNER);
        vault.pause();
        vm.prank(OWNER);
        vault.unpause();

        vm.expectEmit();
        emit Ownable.OwnershipTransferred(OWNER, USER);
        vm.prank(USER);
        vault.acceptOwnership();
        assertEq(vault.owner(), USER, "owner after acceptance");
        assertEq(vault.pendingOwner(), address(0), "pending owner cleared");

        vm.expectRevert(notOwner(OWNER));
        vm.prank(OWNER);
        vault.pause();
        vm.prank(USER);
        vault.pause();
        assertTrue(vault.paused(), "the accepted owner governs");
    }

    function test_OwnershipTransferRefusesForeignAcceptance() public {
        vm.prank(OWNER);
        vault.transferOwnership(USER);
        vm.expectRevert(notOwner(RECIPIENT));
        vm.prank(RECIPIENT);
        vault.acceptOwnership();
        assertEq(vault.owner(), OWNER, "owner unchanged");
        assertEq(vault.pendingOwner(), USER, "pending owner unchanged");
    }

    function test_OwnershipTransferCanBeCancelled() public {
        vm.prank(OWNER);
        vault.transferOwnership(USER);
        vm.prank(OWNER);
        vault.transferOwnership(address(0));
        assertEq(vault.pendingOwner(), address(0), "pending owner cleared");
        vm.expectRevert(notOwner(USER));
        vm.prank(USER);
        vault.acceptOwnership();
        assertEq(vault.owner(), OWNER, "owner unchanged");
    }

    // ---- custody measured as a balance delta ----

    function test_DepositRefusesFeeOnTransferToken() public {
        FeeOnTransferToken fee = new FeeOnTransferToken(100);
        vm.prank(OWNER);
        vault.setCap(address(fee), PER_TX, TOTAL);
        fee.mint(USER, 10 ether);
        vm.prank(USER);
        fee.approve(address(vault), 10 ether);
        vm.expectRevert(abi.encodeWithSelector(PaxeerXVault.TransferAmountMismatch.selector, 9.9 ether, 10 ether));
        vm.prank(USER);
        vault.deposit(address(fee), 10 ether, PAXEER_RECIPIENT);
        assertEq(vault.outstanding(address(fee)), 0, "nothing recorded");
        assertEq(fee.balanceOf(address(vault)), 0, "nothing held");
        assertEq(vault.depositNonce(), 0, "nonce untouched");
    }

    function test_DepositRefusesZeroAmountAndZeroRecipient() public {
        token.mint(USER, 1 ether);
        vm.prank(USER);
        token.approve(address(vault), 1 ether);

        vm.expectRevert(PaxeerXVault.ZeroAmount.selector);
        vm.prank(USER);
        vault.deposit(address(token), 0, PAXEER_RECIPIENT);

        vm.expectRevert(PaxeerXVault.InvalidRecipient.selector);
        vm.prank(USER);
        vault.deposit(address(token), 1 ether, bytes32(0));

        vm.deal(USER, 1 ether);
        vm.expectRevert(PaxeerXVault.ZeroAmount.selector);
        vm.prank(USER);
        vault.depositNative{value: 0}(PAXEER_RECIPIENT);

        vm.expectRevert(PaxeerXVault.InvalidRecipient.selector);
        vm.prank(USER);
        vault.depositNative{value: 1 ether}(bytes32(0));
    }

    // ---- release refusals the wire format keeps ----

    function test_ReleaseRefusesZeroAmountAndZeroRecipient() public {
        depositToken(10 ether);
        bytes32 txHash = keccak256("burn");
        bytes[] memory sigs = releaseSigs(txHash, 1, address(token), 1 ether);

        vm.expectRevert(PaxeerXVault.ZeroAmount.selector);
        vault.release(address(token), 0, RECIPIENT, txHash, 1, sigs);

        vm.expectRevert(PaxeerXVault.InvalidRecipient.selector);
        vault.release(address(token), 1 ether, address(0), txHash, 1, sigs);
    }

    function test_ReleaseRefusesSignatureOfTheWrongLength() public {
        depositToken(10 ether);
        bytes32 txHash = keccak256("burn");
        bytes32 digest = vault.releaseDigest(txHash, 1, RECIPIENT, address(token), 1 ether);
        bytes[] memory sigs = new bytes[](2);
        sigs[0] = new bytes(64);
        sigs[1] = signature(keys[0], digest);
        vm.expectRevert(PaxeerXVault.InvalidSignature.selector);
        vault.release(address(token), 1 ether, RECIPIENT, txHash, 1, sigs);
    }

    function test_ReleaseRefusesRecoveryIdOutsideTwentySevenAndTwentyEight() public {
        depositToken(10 ether);
        bytes32 txHash = keccak256("burn");
        bytes32 digest = vault.releaseDigest(txHash, 1, RECIPIENT, address(token), 1 ether);
        (, bytes32 r, bytes32 s) = vm.sign(keys[0], digest);
        bytes[] memory sigs = new bytes[](2);
        sigs[0] = abi.encodePacked(r, s, uint8(29));
        sigs[1] = signature(keys[1], digest);
        vm.expectRevert(PaxeerXVault.InvalidSignature.selector);
        vault.release(address(token), 1 ether, RECIPIENT, txHash, 1, sigs);
    }

    function test_ReleaseNativeRefusesRecipientThatRejectsValue() public {
        vm.deal(USER, 10 ether);
        vm.prank(USER);
        vault.depositNative{value: 10 ether}(PAXEER_RECIPIENT);
        RejectingRecipient sink = new RejectingRecipient();
        bytes32 txHash = keccak256("burn-native-rejected");
        bytes[] memory sigs = signSorted(twoKeys(), vault.releaseDigest(txHash, 2, address(sink), NATIVE, 1 ether));
        vm.expectRevert(PaxeerXVault.NativeTransferFailed.selector);
        vault.release(NATIVE, 1 ether, address(sink), txHash, 2, sigs);
        assertEq(vault.outstanding(NATIVE), 10 ether, "custody untouched");
        assertTrue(!vault.nullified(vault.nullifierOf(txHash, 2)), "nullifier untouched");
    }

    function test_ReleaseRefusesReentrantToken() public {
        ReentrantToken reentrant = new ReentrantToken();
        vm.prank(OWNER);
        vault.setCap(address(reentrant), PER_TX, TOTAL);
        reentrant.mint(USER, 10 ether);
        vm.prank(USER);
        reentrant.approve(address(vault), 10 ether);
        vm.prank(USER);
        vault.deposit(address(reentrant), 10 ether, PAXEER_RECIPIENT);

        bytes32 outerHash = keccak256("burn-reentrant-outer");
        bytes32 innerHash = keccak256("burn-reentrant-inner");
        bytes[] memory inner =
            signSorted(twoKeys(), vault.releaseDigest(innerHash, 2, RECIPIENT, address(reentrant), 1 ether));
        reentrant.arm(
            address(vault),
            abi.encodeWithSelector(
                PaxeerXVault.release.selector,
                address(reentrant),
                uint256(1 ether),
                RECIPIENT,
                innerHash,
                uint64(2),
                inner
            )
        );
        bytes[] memory outer =
            signSorted(twoKeys(), vault.releaseDigest(outerHash, 1, RECIPIENT, address(reentrant), 1 ether));
        vm.expectRevert(ReentrancyGuard.ReentrancyGuardReentrantCall.selector);
        vault.release(address(reentrant), 1 ether, RECIPIENT, outerHash, 1, outer);
        assertEq(vault.outstanding(address(reentrant)), 10 ether, "custody untouched");
        assertEq(reentrant.balanceOf(RECIPIENT), 0, "nothing paid out");
    }

    // ---- rescue ----

    function test_SetCapRegistersTheAssetOnce() public {
        TestToken other = new TestToken();
        assertTrue(!vault.registered(address(other)), "not registered yet");
        vm.expectEmit();
        emit PaxeerXVault.AssetRegistered(address(other));
        vm.prank(OWNER);
        vault.setCap(address(other), PER_TX, TOTAL);
        assertTrue(vault.registered(address(other)), "registered");

        // A later cap change emits CapSet alone: registration cannot be undone,
        // so zeroing the caps does not open the rescue path onto a bridged asset.
        vm.expectEmit();
        emit PaxeerXVault.CapSet(address(other), 0, 0);
        vm.prank(OWNER);
        vault.setCap(address(other), 0, 0);
        assertTrue(vault.registered(address(other)), "still registered");
        other.mint(address(vault), 1 ether);
        vm.expectRevert(abi.encodeWithSelector(PaxeerXVault.AssetRegisteredForBridging.selector, address(other)));
        vm.prank(OWNER);
        vault.rescue(address(other), OWNER);
    }

    function test_RescueMovesTheWholeBalanceOfAStrayToken() public {
        TestToken stray = new TestToken();
        stray.mint(address(vault), 7 ether);
        assertTrue(!vault.registered(address(stray)), "never registered");
        vm.expectEmit();
        emit PaxeerXVault.Rescued(address(stray), RECIPIENT, 7 ether);
        vm.prank(OWNER);
        vault.rescue(address(stray), RECIPIENT);
        assertEq(stray.balanceOf(RECIPIENT), 7 ether, "rescued in full");
        assertEq(stray.balanceOf(address(vault)), 0, "vault emptied of the stray token");
        assertEq(token.balanceOf(address(vault)), 0, "bridged asset untouched");
    }

    function test_RescueRefusesTheNativeAsset() public {
        vm.deal(address(vault), 3 ether);
        vm.expectRevert(PaxeerXVault.NativeAssetNotRescuable.selector);
        vm.prank(OWNER);
        vault.rescue(NATIVE, RECIPIENT);
        assertEq(address(vault).balance, 3 ether, "native balance untouched");
    }

    function test_RescueRefusesZeroDestination() public {
        TestToken stray = new TestToken();
        stray.mint(address(vault), 1 ether);
        vm.expectRevert(PaxeerXVault.InvalidRecipient.selector);
        vm.prank(OWNER);
        vault.rescue(address(stray), address(0));
        assertEq(stray.balanceOf(address(vault)), 1 ether, "still held");
    }

    function test_RescueRefusesARegisteredToken() public {
        TestToken other = new TestToken();
        vm.prank(OWNER);
        vault.setCap(address(other), PER_TX, TOTAL);
        other.mint(address(vault), 3 ether);
        vm.expectRevert(abi.encodeWithSelector(PaxeerXVault.AssetRegisteredForBridging.selector, address(other)));
        vm.prank(OWNER);
        vault.rescue(address(other), RECIPIENT);
        assertEq(other.balanceOf(address(vault)), 3 ether, "still held");
    }

    function test_RescueRefusesATokenWithOutstandingCustody() public {
        depositToken(12 ether);
        vm.expectRevert(abi.encodeWithSelector(PaxeerXVault.OutstandingNotZero.selector, address(token), 12 ether));
        vm.prank(OWNER);
        vault.rescue(address(token), RECIPIENT);
        assertEq(token.balanceOf(address(vault)), 12 ether, "custody untouched");
        assertEq(vault.outstanding(address(token)), 12 ether, "outstanding untouched");
    }

    function test_RescueRefusesAnEmptyBalance() public {
        TestToken stray = new TestToken();
        vm.expectRevert(abi.encodeWithSelector(PaxeerXVault.NothingToRescue.selector, address(stray)));
        vm.prank(OWNER);
        vault.rescue(address(stray), RECIPIENT);
    }
}

/// @dev Contract with neither a receive nor a fallback function, so a plain
/// value transfer to it fails and must take the whole release down with it.
contract RejectingRecipient {}

/// @dev ERC20 that keeps a fee on every transfer, so the balance that arrives is
/// smaller than the amount asked for. A vault that trusted the requested amount
/// would mint more on Paxeer than it holds in custody.
contract FeeOnTransferToken {
    uint256 public immutable feeBps;
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;
    uint256 public totalSupply;

    constructor(uint256 feeBps_) {
        feeBps = feeBps_;
    }

    function mint(address to, uint256 amount) external {
        balanceOf[to] += amount;
        totalSupply += amount;
    }

    function approve(address spender, uint256 amount) external returns (bool) {
        allowance[msg.sender][spender] = amount;
        return true;
    }

    function transfer(address to, uint256 amount) external returns (bool) {
        _move(msg.sender, to, amount);
        return true;
    }

    function transferFrom(address from, address to, uint256 amount) external returns (bool) {
        allowance[from][msg.sender] -= amount;
        _move(from, to, amount);
        return true;
    }

    function _move(address from, address to, uint256 amount) private {
        uint256 fee = (amount * feeBps) / 10_000;
        balanceOf[from] -= amount;
        balanceOf[to] += amount - fee;
        totalSupply -= fee;
    }
}

/// @dev ERC20 that calls back into a target contract from inside its own
/// transfer, so a reentrant release is attempted through a real token path
/// rather than simulated. The callback fires once and bubbles its revert.
contract ReentrantToken {
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;
    uint256 public totalSupply;
    address public target;
    bytes public callback;

    function mint(address to, uint256 amount) external {
        balanceOf[to] += amount;
        totalSupply += amount;
    }

    function approve(address spender, uint256 amount) external returns (bool) {
        allowance[msg.sender][spender] = amount;
        return true;
    }

    /// @dev Arms the one-shot callback the next transfer makes.
    function arm(address target_, bytes calldata callback_) external {
        target = target_;
        callback = callback_;
    }

    function transfer(address to, uint256 amount) external returns (bool) {
        balanceOf[msg.sender] -= amount;
        balanceOf[to] += amount;
        _fireCallback();
        return true;
    }

    function transferFrom(address from, address to, uint256 amount) external returns (bool) {
        allowance[from][msg.sender] -= amount;
        balanceOf[from] -= amount;
        balanceOf[to] += amount;
        _fireCallback();
        return true;
    }

    function _fireCallback() private {
        address armed = target;
        if (armed == address(0)) return;
        bytes memory data = callback;
        target = address(0);
        (bool ok, bytes memory returned) = armed.call(data);
        if (!ok) {
            assembly ("memory-safe") {
                revert(add(returned, 0x20), mload(returned))
            }
        }
    }
}
