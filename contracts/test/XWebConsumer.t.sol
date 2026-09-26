// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {Test} from "forge-std/Test.sol";
import {IXWeb, IXWebConsumer, XWEB_PRECOMPILE_ADDRESS} from "../src/precompiles/IXWeb.sol";
import {XWebConsumer} from "../src/xweb/XWebConsumer.sol";
import {ApiCall, XWebApi} from "../src/xweb/XWebApi.sol";
import {ApiConsumer} from "../src/xweb/examples/ApiConsumer.sol";

/// The xweb precompile only exists in the node, so its request return is
/// mocked here; the precompile's own behaviour is covered by the Go suite.
contract XWebConsumerTest is Test {
    uint256 internal constant FEE = 1_000_003 * 1e12;
    uint64 internal constant CALLBACK_GAS = 200_000;
    uint8 internal constant KIND_FETCH = 1;
    bytes internal constant PAYLOAD = "https://paxeer.app/";
    bytes32 internal constant DIGEST = bytes32(uint256(0x2222));
    bytes internal constant RESPONSE = "Paxeer X Network";

    XWebConsumer internal consumer;

    event Asked(uint64 indexed requestId, uint8 kind, bytes payload, uint64 callbackGas);
    event Answered(uint64 indexed requestId, bytes32 contentDigest, uint32 fullLength, bytes response);

    function setUp() public {
        consumer = new XWebConsumer();
        vm.deal(address(this), 10 * FEE);
    }

    function ask(uint64 id) internal returns (uint64) {
        vm.mockCall(
            XWEB_PRECOMPILE_ADDRESS,
            FEE,
            abi.encodeCall(IXWeb.request, (KIND_FETCH, PAYLOAD, CALLBACK_GAS)),
            abi.encode(id)
        );
        vm.expectCall(XWEB_PRECOMPILE_ADDRESS, FEE, abi.encodeCall(IXWeb.request, (KIND_FETCH, PAYLOAD, CALLBACK_GAS)));
        vm.expectEmit(true, false, false, true, address(consumer));
        emit Asked(id, KIND_FETCH, PAYLOAD, CALLBACK_GAS);
        return consumer.ask{value: FEE}(KIND_FETCH, PAYLOAD, CALLBACK_GAS);
    }

    function test_AskForwardsTheFeeAndRecordsTheRequest() public {
        assertEq(ask(7), 7);
        assertTrue(consumer.issued(7));
        assertFalse(consumer.issued(8));
    }

    function test_PrecompileDeliversTheAnswerOnce() public {
        ask(7);
        vm.expectEmit(true, false, false, true, address(consumer));
        emit Answered(7, DIGEST, 4096, RESPONSE);
        vm.prank(XWEB_PRECOMPILE_ADDRESS);
        consumer.onXWebResponse(7, DIGEST, 4096, RESPONSE);

        XWebConsumer.Answer memory answer = consumer.answer(7);
        assertTrue(answer.answered);
        assertEq(answer.contentDigest, DIGEST);
        assertEq(answer.fullLength, 4096);
        assertEq(answer.response, RESPONSE);

        vm.prank(XWEB_PRECOMPILE_ADDRESS);
        vm.expectRevert(abi.encodeWithSelector(XWebConsumer.AlreadyAnswered.selector, uint64(7)));
        consumer.onXWebResponse(7, DIGEST, 4096, RESPONSE);
    }

    function test_RefusesAnAnswerFromAnyoneButThePrecompile() public {
        ask(7);
        address intruder = makeAddr("intruder");
        vm.prank(intruder);
        vm.expectRevert(abi.encodeWithSelector(XWebConsumer.NotXWeb.selector, intruder));
        consumer.onXWebResponse(7, DIGEST, 4096, RESPONSE);
        assertFalse(consumer.answer(7).answered);
    }

    function test_RefusesAnAnswerToARequestItDidNotIssue() public {
        ask(7);
        vm.prank(XWEB_PRECOMPILE_ADDRESS);
        vm.expectRevert(abi.encodeWithSelector(XWebConsumer.UnknownRequest.selector, uint64(8)));
        consumer.onXWebResponse(8, DIGEST, 4096, RESPONSE);
    }

    /// The Go suite checks that abi.json carries exactly these signatures.
    function test_InterfaceMatchesThePrecompileABI() public pure {
        assertEq(IXWeb.request.selector, bytes4(keccak256("request(uint8,bytes,uint64)")));
        assertEq(IXWeb.fulfil.selector, bytes4(keccak256("fulfil(uint64,bytes,bytes32,uint32,bytes[])")));
        assertEq(IXWeb.refund.selector, bytes4(keccak256("refund(uint64)")));
        assertEq(IXWeb.getRequest.selector, bytes4(keccak256("getRequest(uint64)")));
        assertEq(IXWeb.getResult.selector, bytes4(keccak256("getResult(uint64)")));
        assertEq(IXWeb.getAttestors.selector, bytes4(keccak256("getAttestors()")));
        assertEq(IXWeb.threshold.selector, bytes4(keccak256("threshold()")));
        assertEq(IXWeb.fee.selector, bytes4(keccak256("fee()")));
        assertEq(IXWeb.getParams.selector, bytes4(keccak256("getParams()")));
        assertEq(
            IXWebConsumer.onXWebResponse.selector, bytes4(keccak256("onXWebResponse(uint64,bytes32,uint32,bytes)"))
        );
        assertEq(
            IXWeb.XWebRequested.selector, keccak256("XWebRequested(uint64,address,uint8,bytes,uint64,uint256,uint64)")
        );
        assertEq(
            IXWeb.XWebFulfilled.selector, keccak256("XWebFulfilled(uint64,address,bytes32,uint32,uint8,uint8,uint64)")
        );
        assertEq(IXWeb.XWebRefunded.selector, keccak256("XWebRefunded(uint64,address,uint256)"));
    }
}

/// Runs XWebApi builders behind an external call so their reverts can be
/// expected.
contract XWebApiHarness {
    function longUrl(uint256 length) external pure returns (bytes memory) {
        return XWebApi.get(string(new bytes(length))).encode();
    }

    function manyPointers(uint256 count) external pure returns (bytes memory) {
        ApiCall memory call = XWebApi.get("https://paxeer.app/");
        for (uint256 i = 0; i < count; i++) {
            call = call.select("/a");
        }
        return call.encode();
    }

    function namesZero() external pure returns (bytes memory) {
        return XWebApi.get("https://paxeer.app/").single(address(0)).encode();
    }
}

/// XWebApi must build byte for byte the api vectors that
/// modules/xweb/types/testdata/api-vectors.json pins for the module.
contract XWebApiTest is Test {
    uint256 internal constant FEE = 1_000_003 * 1e12;
    address internal constant ATTESTOR_1 = 0xE35182c595C7dB5baF6237c02cf6F8177831F384;
    address internal constant ATTESTOR_2 = 0xDd0B8a471d5ADA4D9f7C7f62601460Ed1581C53e;
    string internal constant PRICE_URL = "https://paxeer.app/api/v1/price?asset=PAX";

    bytes internal constant ENVELOPE_ATTESTOR_1_API_KEY =
        hex"e35182c595c7db5baf6237c02cf6f8177831f38403cce31e0a490dd0ff1091c927121efdae3b333ece5fcce941de9cafcc543af48b0102030405060708090a0b0c613d9233ee939f7f2b2566c8fa3118d3b434e6e3880fbb94e32ef684bd2c4b38e19793add7f0355c3623d86cc0dbf6778885c49d0344";
    bytes internal constant ENVELOPE_ATTESTOR_2_API_KEY =
        hex"dd0b8a471d5ada4d9f7c7f62601460ed1581c53e039f0dce916c384faa434208354dbb65adb46d087fb77857a6f790f3295986007f1112131415161718191a1b1c02f5b057f85f6ed7a4f0ee1dfdbec6afe7d191c9ef342b2ac960a31ad6b87757efb982747c97267ce7ad0098f16c95056a48a8579ce8";
    bytes internal constant ENVELOPE_ATTESTOR_1_TWO_HEADERS =
        hex"e35182c595c7db5baf6237c02cf6f8177831f384039e77f530630ee2263c6585bcbeb15d9e399760c8a4a624c8b13107f00e3afba32122232425262728292a2b2cdd0224e37551bab14f6c58bef14eff594d9d0056aefaa76298f90b3b25c05ac4142b5c8993da3ab3ffdd68b98ea114500ef251efcfee55f884354053f3eab1a97c76b4bf5a1e870c5efcb8caa9eb5dbbfe0d3351b6";

    bytes internal constant GET_MAJORITY_SELECT =
        hex"0101000000000000000000000000000000000000000000002968747470733a2f2f7061786565722e6170702f6170692f76312f70726963653f61737365743d50415801000641636365707400106170706c69636174696f6e2f6a736f6e000002000b2f646174612f7072696365000b2f646174612f6173736574020077e35182c595c7db5baf6237c02cf6f8177831f38403cce31e0a490dd0ff1091c927121efdae3b333ece5fcce941de9cafcc543af48b0102030405060708090a0b0c613d9233ee939f7f2b2566c8fa3118d3b434e6e3880fbb94e32ef684bd2c4b38e19793add7f0355c3623d86cc0dbf6778885c49d03440077dd0b8a471d5ada4d9f7c7f62601460ed1581c53e039f0dce916c384faa434208354dbb65adb46d087fb77857a6f790f3295986007f1112131415161718191a1b1c02f5b057f85f6ed7a4f0ee1dfdbec6afe7d191c9ef342b2ac960a31ad6b87757efb982747c97267ce7ad0098f16c95056a48a8579ce8";
    bytes internal constant POST_SINGLE_CREDENTIAL =
        hex"010201e35182c595c7db5baf6237c02cf6f8177831f384001f68747470733a2f2f7061786565722e6170702f6170692f76312f71756f746501000c436f6e74656e742d5479706500106170706c69636174696f6e2f6a736f6e001c7b226173736574223a22534944222c22616d6f756e74223a2231227d01000d2f71756f74652f616d6f756e74010096e35182c595c7db5baf6237c02cf6f8177831f384039e77f530630ee2263c6585bcbeb15d9e399760c8a4a624c8b13107f00e3afba32122232425262728292a2b2cdd0224e37551bab14f6c58bef14eff594d9d0056aefaa76298f90b3b25c05ac4142b5c8993da3ab3ffdd68b98ea114500ef251efcfee55f884354053f3eab1a97c76b4bf5a1e870c5efcb8caa9eb5dbbfe0d3351b6";
    bytes internal constant GET_RAW_BODY =
        hex"0101000000000000000000000000000000000000000000001968747470733a2f2f7061786565722e6170702f7374617475730000000000";
    bytes internal constant EXAMPLE_CONSUMER =
        hex"010101e35182c595c7db5baf6237c02cf6f8177831f384002968747470733a2f2f7061786565722e6170702f6170692f76312f70726963653f61737365743d50415800000001000b2f646174612f7072696365010077e35182c595c7db5baf6237c02cf6f8177831f38403cce31e0a490dd0ff1091c927121efdae3b333ece5fcce941de9cafcc543af48b0102030405060708090a0b0c613d9233ee939f7f2b2566c8fa3118d3b434e6e3880fbb94e32ef684bd2c4b38e19793add7f0355c3623d86cc0dbf6778885c49d0344";

    XWebApiHarness internal harness;

    function setUp() public {
        harness = new XWebApiHarness();
    }

    function test_EncodesTheMajorityVectorWithSelectorsAndEnvelopes() public pure {
        bytes memory payload = XWebApi.get(PRICE_URL).header("Accept", "application/json").select("/data/price")
            .select("/data/asset").withCredential(ENVELOPE_ATTESTOR_1_API_KEY)
            .withCredential(ENVELOPE_ATTESTOR_2_API_KEY).encode();
        assertEq(payload, GET_MAJORITY_SELECT);
    }

    function test_EncodesTheSingleLevelPostVector() public pure {
        bytes memory payload = XWebApi.post("https://paxeer.app/api/v1/quote", bytes('{"asset":"SID","amount":"1"}'))
            .header("Content-Type", "application/json").select("/quote/amount")
            .withCredential(ENVELOPE_ATTESTOR_1_TWO_HEADERS).single(ATTESTOR_1).encode();
        assertEq(payload, POST_SINGLE_CREDENTIAL);
    }

    function test_EncodesTheRawBodyVector() public pure {
        assertEq(XWebApi.get("https://paxeer.app/status").encode(), GET_RAW_BODY);
    }

    function test_EncodesTheExampleConsumerVector() public pure {
        bytes memory payload = XWebApi.get(PRICE_URL).select("/data/price").withCredential(ENVELOPE_ATTESTOR_1_API_KEY)
            .single(ATTESTOR_1).encode();
        assertEq(payload, EXAMPLE_CONSUMER);
    }

    function test_ApiConsumerSubmitsTheExamplePayloadAtTheFee() public {
        ApiConsumer consumer = new ApiConsumer();
        bytes memory call = abi.encodeCall(IXWeb.request, (XWebApi.KIND_API, EXAMPLE_CONSUMER, uint64(200_000)));
        vm.mockCall(XWEB_PRECOMPILE_ADDRESS, abi.encodeCall(IXWeb.fee, ()), abi.encode(FEE));
        vm.mockCall(XWEB_PRECOMPILE_ADDRESS, FEE, call, abi.encode(uint64(11)));
        vm.expectCall(XWEB_PRECOMPILE_ADDRESS, FEE, call);
        vm.deal(address(this), FEE);
        assertEq(consumer.askPrice{value: FEE}(ENVELOPE_ATTESTOR_1_API_KEY, ATTESTOR_1), 11);

        vm.prank(XWEB_PRECOMPILE_ADDRESS);
        consumer.onXWebResponse(11, bytes32(0), 5, "3.114");
        assertEq(consumer.prices(11), bytes("3.114"));

        vm.expectRevert(bytes("only xweb"));
        consumer.onXWebResponse(11, bytes32(0), 5, "0");
    }

    function test_RefusesAFieldOverTheUint16Length() public {
        vm.expectRevert(abi.encodeWithSelector(XWebApi.FieldTooLong.selector, "url", uint256(65_536)));
        harness.longUrl(65_536);
        assertEq(harness.longUrl(65_535).length, 3 + 20 + 2 + 65_535 + 1 + 2 + 1 + 1);
    }

    function test_RefusesAFieldCountOverOneByte() public {
        assertEq(harness.manyPointers(255)[25 + 19 + 1 + 2], bytes1(0xff));
        vm.expectRevert(abi.encodeWithSelector(XWebApi.TooMany.selector, "pointers"));
        harness.manyPointers(256);
    }

    function test_RefusesASingleLevelNamingNoAttestor() public {
        vm.expectRevert(XWebApi.ZeroAttestor.selector);
        harness.namesZero();
    }

    function test_SecondAttestorEnvelopeIsAddressedToIt() public pure {
        assertEq(address(bytes20(ENVELOPE_ATTESTOR_2_API_KEY)), ATTESTOR_2);
        assertEq(address(bytes20(ENVELOPE_ATTESTOR_1_API_KEY)), ATTESTOR_1);
    }
}
