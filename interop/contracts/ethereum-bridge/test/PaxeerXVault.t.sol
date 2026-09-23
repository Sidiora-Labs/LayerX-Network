// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.30;

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

    function test_OwnerOnlySetters() public {
        address[] memory attestors = new address[](1);
        attestors[0] = vm.addr(OUTSIDER_KEY);

        vm.expectRevert(PaxeerXVault.NotOwner.selector);
        vm.prank(USER);
        vault.setAttestors(attestors, 1);

        vm.expectRevert(PaxeerXVault.NotOwner.selector);
        vm.prank(USER);
        vault.setCap(address(token), 1, 1);

        vm.expectRevert(PaxeerXVault.NotOwner.selector);
        vm.prank(USER);
        vault.pause();

        vm.prank(OWNER);
        vault.pause();
        vm.expectRevert(PaxeerXVault.NotOwner.selector);
        vm.prank(USER);
        vault.unpause();

        vm.expectRevert(PaxeerXVault.NotOwner.selector);
        vm.prank(USER);
        vault.transferOwnership(USER);
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
}
