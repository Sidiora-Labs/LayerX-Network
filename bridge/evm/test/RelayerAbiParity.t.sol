// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.30;

import {PaxeerXVault} from "../src/PaxeerXVault.sol";
import {CheatTest} from "./Vm.sol";

/// @notice Pins the vault's wire format to the byte values the relayer compiles
/// in. The selectors and topics are read off a real deployed vault, so renaming
/// an event, reordering a parameter or widening a type fails here rather than at
/// run time against a vault that already holds custody.
contract RelayerAbiParityTest is CheatTest {
    /// @dev abi.rs `BRIDGE_DEPOSIT_TOPIC`: the topic the relayer filters
    /// `eth_getLogs` on and matches every observed vault log against.
    bytes32 internal constant BRIDGE_DEPOSIT_TOPIC = 0x19a8713a4594d98824323571a687838aa76b4d730cfedde1785ee8bb92195190;
    /// @dev abi.rs `RELEASE_SELECTOR`: the first four bytes of the calldata
    /// `encode_release` builds for an outbound attestation.
    bytes4 internal constant RELEASE_SELECTOR = 0x05eb766d;
    /// @dev abi.rs `THRESHOLD_SELECTOR`.
    bytes4 internal constant THRESHOLD_SELECTOR = 0x42cde4e8;
    /// @dev abi.rs `ATTESTORS_SELECTOR`.
    bytes4 internal constant ATTESTORS_SELECTOR = 0xe7eb466f;
    /// @dev abi.rs `NULLIFIED_SELECTOR`.
    bytes4 internal constant NULLIFIED_SELECTOR = 0xe73bdb5e;

    /// @dev The rest of the wire format the relayer, the attestors and the
    /// Paxeer side depend on. abi.rs declares no constant for these, so this
    /// test is their byte-level record: the two deposit entry points, the
    /// release event, and the two digest views an attestor checks a digest
    /// against before signing it.
    bytes32 internal constant BRIDGE_RELEASE_TOPIC = 0x992987ddbf5f2e96efbe31912c21fcf3c7acad0a10b66d59cb6c8a6760b7f393;
    bytes4 internal constant DEPOSIT_SELECTOR = 0x26b3293f;
    bytes4 internal constant DEPOSIT_NATIVE_SELECTOR = 0x42ef5fbb;
    bytes4 internal constant RELEASE_DIGEST_SELECTOR = 0x90a99162;
    bytes4 internal constant DEPOSIT_DIGEST_SELECTOR = 0x60bb721d;

    PaxeerXVault internal vault;

    function setUp() public {
        address[] memory attestors = new address[](1);
        attestors[0] = vm.addr(0xA1);
        vault = new PaxeerXVault(address(0xB0B), attestors, 1);
    }

    function test_EventTopicsMatchTheRelayerConstants() public pure {
        assertEq(PaxeerXVault.BridgeDeposit.selector, BRIDGE_DEPOSIT_TOPIC, "BridgeDeposit topic");
        assertEq(PaxeerXVault.BridgeRelease.selector, BRIDGE_RELEASE_TOPIC, "BridgeRelease topic");
    }

    function test_EventTopicsAreTheKeccakOfTheirSignatures() public pure {
        assertEq(
            keccak256("BridgeDeposit(address,uint256,address,bytes32,uint64)"),
            BRIDGE_DEPOSIT_TOPIC,
            "BridgeDeposit signature"
        );
        assertEq(
            keccak256("BridgeRelease(address,uint256,address,bytes32,uint64)"),
            BRIDGE_RELEASE_TOPIC,
            "BridgeRelease signature"
        );
    }

    function test_SelectorsMatchTheRelayerConstants() public view {
        assertEq(selectorOf(vault.deposit.selector), selectorOf(DEPOSIT_SELECTOR), "deposit selector");
        assertEq(selectorOf(vault.depositNative.selector), selectorOf(DEPOSIT_NATIVE_SELECTOR), "depositNative");
        assertEq(selectorOf(vault.release.selector), selectorOf(RELEASE_SELECTOR), "release selector");
        assertEq(selectorOf(vault.releaseDigest.selector), selectorOf(RELEASE_DIGEST_SELECTOR), "releaseDigest");
        assertEq(selectorOf(vault.depositDigest.selector), selectorOf(DEPOSIT_DIGEST_SELECTOR), "depositDigest");
        assertEq(selectorOf(vault.threshold.selector), selectorOf(THRESHOLD_SELECTOR), "threshold selector");
        assertEq(selectorOf(vault.attestors.selector), selectorOf(ATTESTORS_SELECTOR), "attestors selector");
        assertEq(selectorOf(vault.nullified.selector), selectorOf(NULLIFIED_SELECTOR), "nullified selector");
    }

    function test_SelectorsAreTheKeccakOfTheirSignatures() public pure {
        assertEq(
            selectorOf(bytes4(keccak256("deposit(address,uint256,bytes32)"))),
            selectorOf(DEPOSIT_SELECTOR),
            "deposit signature"
        );
        assertEq(
            selectorOf(bytes4(keccak256("depositNative(bytes32)"))),
            selectorOf(DEPOSIT_NATIVE_SELECTOR),
            "depositNative signature"
        );
        assertEq(
            selectorOf(bytes4(keccak256("release(address,uint256,address,bytes32,uint64,bytes[])"))),
            selectorOf(RELEASE_SELECTOR),
            "release signature"
        );
        assertEq(
            selectorOf(bytes4(keccak256("releaseDigest(bytes32,uint64,address,address,uint256)"))),
            selectorOf(RELEASE_DIGEST_SELECTOR),
            "releaseDigest signature"
        );
        assertEq(
            selectorOf(bytes4(keccak256("depositDigest(bytes32,uint64,bytes32,address,uint256)"))),
            selectorOf(DEPOSIT_DIGEST_SELECTOR),
            "depositDigest signature"
        );
        assertEq(selectorOf(bytes4(keccak256("threshold()"))), selectorOf(THRESHOLD_SELECTOR), "threshold signature");
        assertEq(selectorOf(bytes4(keccak256("attestors()"))), selectorOf(ATTESTORS_SELECTOR), "attestors signature");
        assertEq(
            selectorOf(bytes4(keccak256("nullified(bytes32)"))), selectorOf(NULLIFIED_SELECTOR), "nullified signature"
        );
    }

    /// @dev The vault keeps no EIP-165 surface, so nothing answers
    /// supportsInterface and the relayer never asks.
    function test_VaultHasNoEip165Surface() public {
        (bool ok,) = address(vault).call(abi.encodeWithSelector(bytes4(0x01ffc9a7), bytes4(0x01ffc9a7)));
        assertTrue(!ok, "supportsInterface must not answer");
    }

    function selectorOf(bytes4 selector) internal pure returns (bytes32) {
        return bytes32(selector);
    }
}
