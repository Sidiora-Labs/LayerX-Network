// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {Arithmetic} from "../contracts/libraries/Arithmetic.sol";
import {Bytes} from "../contracts/libraries/Bytes.sol";
import {Constants} from "../contracts/libraries/Constants.sol";
import {CryptographyPrimitives} from "../contracts/libraries/CryptographyPrimitives.sol";
import {DecimalsConverterHelper} from "../contracts/libraries/DecimalsConverterHelper.sol";
import {Encoding} from "../contracts/libraries/Encoding.sol";
import {Error} from "../contracts/libraries/Error.sol";
import {Hashing} from "../contracts/libraries/Hashing.sol";
import {MerkleLib} from "../contracts/libraries/MerkleLib.sol";
import {Types} from "../contracts/libraries/Types.sol";

interface PrimitiveVm {
    function addr(uint256 privateKey) external returns (address);
    function assume(bool condition) external;
    function expectRevert(bytes4 selector) external;
    function expectPartialRevert(bytes4 selector) external;
    function sign(uint256 privateKey, bytes32 digest) external returns (uint8 v, bytes32 r, bytes32 s);
}

contract PrimitiveHarness {
    function add(uint256 a, uint256 b) external pure returns (uint256) {
        return Arithmetic.add(a, b);
    }

    function mul(uint256 a, uint256 b) external pure returns (uint256) {
        return Arithmetic.mul(a, b);
    }

    function narrow128(uint256 value) external pure returns (uint128) {
        return Arithmetic.toUint128(value);
    }

    function mulDiv(uint256 x, uint256 y, uint256 denominator, bool roundUp) external pure returns (uint256) {
        return Arithmetic.mulDiv(x, y, denominator, roundUp ? Types.Rounding.Up : Types.Rounding.Down);
    }

    function parse(bytes calldata value) external pure returns (uint16, uint32, uint64, bytes32, address) {
        return (
            Bytes.readUint16BE(value, 0),
            Bytes.readUint32BE(value, 2),
            Bytes.readUint64BE(value, 6),
            Bytes.readBytes32(value, 14),
            Bytes.readAddress(value, 46)
        );
    }

    function readWord(bytes calldata value, uint256 offset) external pure returns (bytes32) {
        return Bytes.readBytes32(value, offset);
    }

    function hash(bytes32 domain, bytes calldata value) external pure returns (bytes32, bytes32) {
        return (Hashing.keccakDomain(domain, value), Hashing.sha256Domain(domain, value));
    }

    function encodedCall(Types.CallCommitment calldata operation) external pure returns (bytes32) {
        return Encoding.callCommitment(operation);
    }

    function recover(bytes32 digest, bytes calldata signature) external pure returns (address) {
        return CryptographyPrimitives.recover(digest, signature);
    }

    function proofRoot(bytes32 leaf, uint256 index, bytes32[] calldata siblings) external pure returns (bytes32) {
        return MerkleLib.root(leaf, index, siblings);
    }

    function convert(uint256 amount, uint8 from, uint8 to)
        external
        pure
        returns (uint256 converted, uint256 remainder)
    {
        Types.DecimalConversion memory result = DecimalsConverterHelper.convert(amount, from, to);
        return (result.converted, result.remainder);
    }

    function convertExact(uint256 amount, uint8 from, uint8 to) external pure returns (uint256) {
        return DecimalsConverterHelper.convertExact(amount, from, to);
    }

    function errorMetadata(address target, bytes calldata returnData)
        external
        pure
        returns (bytes4 selector, Error.Kind kind, bytes32 commitment)
    {
        bytes memory data = returnData;
        return (Error.selector(data), Error.classify(data), Error.commitment(target, data));
    }
}

contract ContractPrimitivesTest {
    PrimitiveVm private constant vm = PrimitiveVm(address(uint160(uint256(keccak256("hevm cheat code")))));
    PrimitiveHarness private harness;

    function setUp() public {
        harness = new PrimitiveHarness();
    }

    function testCheckedArithmeticAndFullPrecisionMulDiv() public view {
        require(harness.add(4, 5) == 9, "add");
        require(harness.mul(7, 8) == 56, "mul");
        require(harness.mulDiv(type(uint256).max, 9, type(uint256).max, false) == 9, "full precision");
        require(harness.mulDiv(10, 10, 6, false) == 16, "round down");
        require(harness.mulDiv(10, 10, 6, true) == 17, "round up");
    }

    function testArithmeticRejectsOverflowAndNarrowing() public {
        vm.expectRevert(Arithmetic.ArithmeticOverflow.selector);
        harness.add(type(uint256).max, 1);
        vm.expectRevert(Arithmetic.ArithmeticOverflow.selector);
        harness.mul(type(uint256).max, 2);
        vm.expectRevert(Arithmetic.NarrowingConversion.selector);
        harness.narrow128(uint256(type(uint128).max) + 1);
    }

    function testBoundedBigEndianReaders() public view {
        bytes memory value = abi.encodePacked(
            uint16(0x1234),
            uint32(0x56789abc),
            uint64(0xdef0123456789abc),
            bytes32(uint256(0x55)),
            address(0x1234567890123456789012345678901234567890)
        );
        (uint16 a, uint32 b, uint64 c, bytes32 d, address e) = harness.parse(value);
        require(a == 0x1234 && b == 0x56789abc && c == 0xdef0123456789abc, "integers");
        require(d == bytes32(uint256(0x55)), "bytes32");
        require(e == address(0x1234567890123456789012345678901234567890), "address");
    }

    function testReaderRejectsOutOfBounds() public {
        vm.expectPartialRevert(Bytes.BytesOutOfBounds.selector);
        harness.readWord(hex"0102", 0);
    }

    function testDomainSeparatedHashesAndCallCommitments() public view {
        (bytes32 firstKeccak, bytes32 firstSha) = harness.hash(bytes32(uint256(1)), hex"0102");
        (bytes32 secondKeccak, bytes32 secondSha) = harness.hash(bytes32(uint256(2)), hex"0102");
        require(firstKeccak != secondKeccak && firstSha != secondSha, "domain collision");
        Types.CallCommitment memory operation = Types.CallCommitment({
            target: address(0x1234),
            value: 5,
            dataHash: keccak256("data"),
            expectedCodeHashBefore: keccak256("before"),
            expectedCodeHashAfter: keccak256("after")
        });
        bytes32 commitment = harness.encodedCall(operation);
        operation.value = 6;
        require(commitment != harness.encodedCall(operation), "call field not bound");
    }

    function testCanonicalErrorClassificationAndCommitment() public view {
        address target = address(0xCA11);
        bytes memory revertString = abi.encodeWithSignature("Error(string)", "denied");
        (bytes4 selector, Error.Kind kind, bytes32 firstCommitment) = harness.errorMetadata(target, revertString);
        require(selector == Error.REVERT_STRING_SELECTOR, "revert selector");
        require(kind == Error.Kind.RevertString, "revert classification");

        bytes memory panic = abi.encodeWithSelector(Error.PANIC_SELECTOR, uint256(0x11));
        (selector, kind,) = harness.errorMetadata(target, panic);
        require(selector == Error.PANIC_SELECTOR && kind == Error.Kind.Panic, "panic classification");

        bytes memory custom = abi.encodeWithSelector(Arithmetic.ArithmeticOverflow.selector);
        (selector, kind,) = harness.errorMetadata(target, custom);
        require(
            selector == Arithmetic.ArithmeticOverflow.selector && kind == Error.Kind.Custom, "custom classification"
        );

        (selector, kind,) = harness.errorMetadata(target, hex"010203");
        require(selector == bytes4(0) && kind == Error.Kind.Empty, "short classification");
        (,, bytes32 secondCommitment) = harness.errorMetadata(address(0xCA12), revertString);
        require(firstCommitment != secondCommitment, "error target not bound");
    }

    function testStrictSignatureRecoveryAndMalleabilityRejection() public {
        uint256 privateKey = 777;
        bytes32 digest = keccak256("LayerX primitive test");
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(privateKey, digest);
        bytes memory signature = abi.encodePacked(r, s, v);
        require(harness.recover(digest, signature) == vm.addr(privateKey), "recovery");
        uint256 highS = Constants.SECP256K1_ORDER - uint256(s);
        bytes memory malleated = abi.encodePacked(r, bytes32(highS), v == 27 ? uint8(28) : uint8(27));
        vm.expectPartialRevert(CryptographyPrimitives.InvalidSignatureS.selector);
        harness.recover(digest, malleated);
    }

    function testSignatureRecoveryRejectsNonCanonicalLength() public {
        vm.expectPartialRevert(CryptographyPrimitives.InvalidSignatureLength.selector);
        harness.recover(keccak256("digest"), new bytes(63));
    }

    function testIndexAwareMerkleProof() public view {
        bytes32 left = MerkleLib.hashLeaf("left");
        bytes32 right = MerkleLib.hashLeaf("right");
        bytes32[] memory siblings = new bytes32[](1);
        siblings[0] = left;
        bytes32 expected = MerkleLib.hashNode(left, right);
        require(harness.proofRoot(right, 1, siblings) == expected, "proof");
    }

    function testMerkleRejectsIndexOutsideProofDepth() public {
        bytes32[] memory siblings = new bytes32[](1);
        siblings[0] = bytes32(uint256(1));
        vm.expectPartialRevert(MerkleLib.MerkleIndexOutOfRange.selector);
        harness.proofRoot(bytes32(uint256(2)), 2, siblings);
    }

    function testDecimalConversionNeverHidesDust() public {
        (uint256 down, uint256 remainder) = harness.convert(1_234_567, 6, 4);
        require(down == 12_345 && remainder == 67, "downscale");
        require(harness.convertExact(12_340_000, 6, 4) == 123_400, "exact");
        vm.expectPartialRevert(DecimalsConverterHelper.InexactConversion.selector);
        harness.convertExact(1_234_567, 6, 4);
    }

    function testFuzz_DecimalRoundTripExact(uint128 amount, uint8 decimals) public {
        vm.assume(decimals <= 38);
        uint256 factor = DecimalsConverterHelper.scaleFactor(decimals);
        vm.assume(uint256(amount) <= type(uint256).max / factor);
        uint256 expanded = harness.convertExact(amount, 0, decimals);
        require(harness.convertExact(expanded, decimals, 0) == amount, "round trip");
    }

    function testFuzz_MulDivMatchesNonOverflowingProduct(uint128 x, uint128 y, uint128 denominator) public {
        vm.assume(denominator != 0);
        uint256 product = uint256(x) * uint256(y);
        require(harness.mulDiv(x, y, denominator, false) == product / denominator, "mulDiv mismatch");
    }

    function testFuzz_BytesRejectEveryShortWord(bytes memory value) public {
        vm.assume(value.length < 32);
        vm.expectPartialRevert(Bytes.BytesOutOfBounds.selector);
        harness.readWord(value, 0);
    }

    function testFuzz_HashBindsDomain(bytes32 firstDomain, bytes32 secondDomain, bytes memory value) public {
        vm.assume(firstDomain != secondDomain);
        vm.assume(value.length <= 4096);
        (bytes32 firstKeccak, bytes32 firstSha) = harness.hash(firstDomain, value);
        (bytes32 secondKeccak, bytes32 secondSha) = harness.hash(secondDomain, value);
        require(firstKeccak != secondKeccak && firstSha != secondSha, "domain not bound");
    }
}
